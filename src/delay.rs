//! Busy-wait delays on the core SysTick counter.
//!
//! Most CH5xx dies have the WCH up-counter variant: `CTLR` (`STE` bit 0,
//! `STCLK` bit 2, `MODE` bit 4), `SR.CNTIF`, and the 32-bit `CNTL` / `CMPLR`
//! pair. CH573 instead has the V3A core timer: a 64-bit **down-counter** with
//! a one-shot `STRELOAD` load and the flag in `CNTFG`. Both are started as a
//! free-running counter here; the direction only changes how elapsed time is
//! computed.
//!
//! The counter is started **once** by `init` and left running; each delay
//! only reads `CNT` and compares it against a target.
//!
//! # Why the rate is measured, not assumed
//!
//! The clock tree now gives an exact nominal `Fsys` ([`crate::rcc::Config::sysclk`]),
//! so the rate could be derived instead. It is measured anyway because the RC
//! oscillators are not exact and a wrong constant makes every delay wrong by
//! the same factor.
//!
//! `init` calibrates the counter against the 32 kHz RTC counter, the same
//! trick WCH's `OSC_CAL` uses. The result drives [`Delay`] and is readable as
//! [`measured_sysclk`]. It is a measurement, so it varies slightly between
//! resets because the internal 32 kHz RC is the reference.
//!
//! [`crate::rcc::clocks`] reports the nominal frequency of the configured clock
//! tree instead. For HSE/PLL that is more accurate than a measurement
//! referenced to an RC oscillator, and it keeps the value stable across resets.
//!
//! Note that `probe-rs read` must not be used to check this: it halts the core
//! and reports values the running firmware does not see.
//!
//! # What it counts on
//!
//! Without a time driver, [`Delay`] polls the raw SysTick counter directly —
//! the cheapest option.
//!
//! Under `time-driver-systick` the embassy driver owns the counter and
//! reprograms its compare for every alarm, so raw polling would measure
//! against a counter that restarts underneath it (measured on hardware with
//! an alarm every 100 ms, `delay_ms(1000)` took 97 ms). [`Delay::delay_us`]
//! therefore chases the raw counter and falls back to
//! [`embassy_time_driver::now`] when the counter moves underneath it;
//! [`Delay::delay_ms`] reads `now()` throughout. Both are one embassy tick of
//! quantization, rounded up. An executor that never sleeps hides the problem
//! entirely: raw polling looks fine until the first task actually sleeps.
//!
//! The rate measurement does not depend on any of this: `init` runs under
//! every feature, before the time driver takes the counter over.

use crate::atomic::Ordering;
use crate::pac::SYSTICK;

/// A delay backed by the SysTick timebase.
///
/// Requires [`crate::init`] to have run, so the counter is started and its rate
/// is known.
pub struct Delay;

/// SysTick ticks per second, measured against the RTC at init.
static TICKS_PER_SEC: crate::atomic::AtomicU32 = crate::atomic::AtomicU32::new(crate::rcc::DEFAULT_FREQUENCY.0);

/// Ticks in one microsecond, Q16.16 fixed point, rounded up.
///
/// Not stored for the CH573 down-counter SysTick driver, which never chases
/// raw ticks.
#[cfg(not(all(feature = "time-driver-systick", systick_downcounter)))]
const fn ticks_per_us_q16(hz: u32) -> u32 {
    ((((hz as u64) << 16) + 999_999) / 1_000_000) as u32
}

/// [`ticks_per_us_q16`] for the measured rate, so `delay_us` needs no division.
#[cfg(not(all(feature = "time-driver-systick", systick_downcounter)))]
static TICKS_PER_US_Q16: crate::atomic::AtomicU32 =
    crate::atomic::AtomicU32::new(ticks_per_us_q16(crate::rcc::DEFAULT_FREQUENCY.0));

/// RTC block, whose `CNT_32K` counter is clocked by the independent 32 kHz
/// oscillator and keeps running across anything the PLL does.
const RTC_BASE: u32 = 0x4000_1000;

/// RTC count used for the calibration window.
///
/// 4096 counts at 32.768 kHz is 125 ms. A shorter window is too sensitive to
/// the skew between the `CNT_32K` and `CNTL` reads: at 1024 counts the
/// same board measured anywhere from 5.33 to 7.43 MHz run to run, which then
/// scaled every delay (and the whole embassy timebase) by up to 40%.
const CAL_COUNTS: u16 = 4096;

/// Calibration rounds; the median is used so a single disturbed window cannot
/// skew the result.
const CAL_ROUNDS: usize = 5;

/// RTC frequency, for turning counts into seconds.
const RTC_HZ: u32 = 32_768;

/// SysTick counts elapsed between two `CNTL` reads.
///
/// Up-counters (`MODE = 0`) advance above `start`; CH573's V3A down-counter
/// falls below it, so the subtraction is reversed.
#[inline(always)]
fn elapsed_ticks(start: u32, end: u32) -> u32 {
    #[cfg(systick_downcounter)]
    {
        start.wrapping_sub(end)
    }
    #[cfg(not(systick_downcounter))]
    {
        end.wrapping_sub(start)
    }
}

#[inline(always)]
fn rtc_count() -> u16 {
    // `CNT_32K` is the 16-bit register at 0x38. It wraps every 2 s, but the
    // calibration window is at least 125 ms, so the low half alone is enough —
    // `wrapping_sub` handles the one wrap that could occur.
    unsafe { core::ptr::read_volatile((RTC_BASE + 0x38) as *const u16) }
}

/// Start SysTick as a free-running counter and measure its rate.
///
/// Runs on every build, even when the SysTick time driver owns the counter.
/// The measurement is this module's private business: the driver and every
/// other consumer read the nominal frequency from [`crate::rcc::clocks`]. When
/// the driver is compiled in, its `init` runs directly after this and
/// reprograms the counter for its own reload model.
///
/// # Safety
///
/// Call once, from [`crate::init`].
pub(crate) unsafe fn init() {
    // Free-running, HCLK source (`STCLK` set — clearing it would select
    // HCLK/8). `STE` starts it. CH573 keeps the flag in `CNTFG` and has no
    // counter-direction bit; its 64-bit counter starts at the maximum value
    // and counts down, so a zero-crossing is ~12,000 years away instead of
    // firing right after `init`.
    cfg_select! {
        systick_downcounter => {
            SYSTICK.cntfg().modify(|w| w.set_cntif(false));
        }
        systick_cntfg => {
            SYSTICK.cntfg().modify(|w| w.set_cntif(false));
        }
        _ => {
            SYSTICK.sr().modify(|w| w.set_cntif(false));
        }
    }
    cfg_select! {
        systick_downcounter => {
            SYSTICK.cntl().write(|w| w.set_cntl(u32::MAX));
            SYSTICK.cnth().write(|w| w.set_cnth(u32::MAX));
        }
        _ => {
            SYSTICK.cntl().write(|w| w.set_cntl(0));
            // CH572 has a single 32-bit counter register and no high half.
            #[cfg(systick_cnth)]
            SYSTICK.cnth().write(|w| w.set_cnth(0));
        }
    }
    SYSTICK.ctlr().modify(|w| {
        #[cfg(systick_mode)]
        w.set_mode(false); // up-count
        w.set_stclk(true); // source: HCLK (not HCLK/8)
        w.set_stie(false); // poll, never interrupt
        w.set_ste(true);
    });

    measure();
}

/// Re-measure the SysTick rate after [`crate::rcc::set_config`] changed `Fsys`.
///
/// Does not start or stop the counter, so it can be used after a runtime clock
/// change. It is only meaningful while SysTick is the free-running counter this
/// module set up: under `time-driver-systick` the embassy driver reloads it for
/// every alarm, which invalidates the measurement, and [`Delay`] uses the
/// driver's timebase instead.
///
/// # Safety
///
/// The caller must have applied a clock tree with [`crate::rcc::set_config`].
#[cfg(not(feature = "time-driver-systick"))]
pub unsafe fn recalibrate() {
    measure();
}

/// Measure the counter and store the result for the busy-wait loops.
unsafe fn measure() {
    let hz = calibrate();
    TICKS_PER_SEC.store(hz, Ordering::Relaxed);
    #[cfg(not(all(feature = "time-driver-systick", systick_downcounter)))]
    TICKS_PER_US_Q16.store(ticks_per_us_q16(hz), Ordering::Relaxed);
}

/// The SysTick rate this module measured at [`crate::init`] or at the last
/// `recalibrate`.
///
/// This is what [`Delay`] uses. [`crate::rcc::clocks`] reports the nominal rate
/// of the configured tree instead; the two differ by the RC oscillator error.
pub fn measured_sysclk() -> crate::time::Hertz {
    crate::time::Hertz(TICKS_PER_SEC.load(Ordering::Relaxed))
}

#[cfg(not(feature = "time-driver-systick"))]
impl Delay {
    /// Busy-wait for `us` microseconds.
    ///
    /// Panics in debug builds if called before [`crate::init`].
    ///
    /// Under a TMR time driver (or none) this module owns the raw counter,
    /// so this is the same hot chase loop as under the SysTick driver.
    #[cfg_attr(feature = "highcode", inline(never), link_section = ".highcode")]
    pub fn delay_us(&mut self, us: u32) {
        let q16 = TICKS_PER_US_Q16.load(Ordering::Relaxed) as u64;
        debug_assert!(q16 != 0, "delay used before `ch5xx_hal::init`");

        // Q16 budget, rounded up, so the wait never comes back short. `u64`
        // because a `u32` cast would truncate delays past one counter wrap.
        let mut remaining = (us as u64 * q16 + 0xFFFF) >> 16;

        // The counter is 32 bits, so chase it in chunks to keep the
        // wrapping comparison meaningful.
        while remaining > 0 {
            let chunk = remaining.min(u64::from(u32::MAX / 2)) as u32;
            remaining -= chunk as u64;

            let start = SYSTICK.cntl().read().cntl();
            while elapsed_ticks(start, SYSTICK.cntl().read().cntl()) < chunk {
                core::hint::spin_loop();
            }
        }
    }

    /// Busy-wait for `ms` milliseconds.
    pub fn delay_ms(&mut self, ms: u32) {
        // Split so a large `ms` cannot overflow the microsecond multiply.
        let mut ms = ms;
        while ms > 4_000 {
            self.delay_us(4_000_000);
            ms -= 4_000;
        }
        self.delay_us(ms * 1_000);
    }
}

/// Under the SysTick time driver the counter's compare point belongs to the
/// driver (see the module docs). `delay_us` chases the raw counter and falls
/// back to `now()`; `delay_ms` reads `now()` throughout — expensive on the
/// V3C, where each poll runs software divisions (tens of µs).
///
/// The two `delay_us` bodies are cfg-exclusive: the up-counter chases the raw
/// register, the CH573 down-counter's one-shot `STRELOAD` restarts the counter
/// at every alarm, so it always reads the timebase.
#[cfg(feature = "time-driver-systick")]
impl Delay {
    /// Busy-wait for `ms` milliseconds.
    ///
    /// Reads the timebase through `now()`; the ~tens-of-µs poll cost is
    /// amortized over the wait and the deadline logic stays simple.
    pub fn delay_ms(&mut self, ms: u32) {
        let ticks = (embassy_time_driver::TICK_HZ * ms as u64).div_ceil(1_000);
        self.wait_ticks(ticks);
    }

    /// Wait for `ticks` driver ticks on the `now()` deadline.
    fn wait_ticks(&mut self, ticks: u64) {
        let deadline = embassy_time_driver::now().saturating_add(ticks);
        while embassy_time_driver::now() < deadline {
            core::hint::spin_loop();
        }
    }

    /// Busy-wait for `us` microseconds (up-counter dies).
    ///
    /// Budgets the wait in raw counter ticks up front — Q16 rate, rounded up,
    /// so it never comes back short — and the loop only subtracts. If the
    /// counter wraps or the time driver reloads it, the remainder finishes on
    /// the driver timebase in one round-up via a cold fallback.
    ///
    /// Caveat: a reload is only caught if observed across two reads. An
    /// interrupt preempting the loop for a full reload period can hide it,
    /// under-counting by up to one period; route long waits through
    /// [`Self::delay_ms`], which reads the monotonic timebase throughout.
    #[cfg(not(systick_downcounter))]
    #[cfg_attr(feature = "highcode", inline(never), link_section = ".highcode")]
    pub fn delay_us(&mut self, us: u32) {
        let q16 = TICKS_PER_US_Q16.load(Ordering::Relaxed) as u64;
        let mut remaining = ((us as u64 * q16 + 0xFFFF) >> 16) as u64;
        let mut prev = SYSTICK.cntl().read().cntl();
        loop {
            let now = SYSTICK.cntl().read().cntl();
            if now < prev {
                self.finish_on_timebase(remaining);
                return;
            }
            remaining = remaining.saturating_sub(elapsed_ticks(prev, now) as u64);
            prev = now;
            if remaining == 0 {
                return;
            }
        }
    }

    /// Cold fallback: convert the raw-tick remainder to driver ticks, one
    /// round-up. Stays out of `.highcode`.
    #[cfg(not(systick_downcounter))]
    #[cold]
    #[inline(never)]
    fn finish_on_timebase(&mut self, remaining: u64) {
        let hz = TICKS_PER_SEC.load(Ordering::Relaxed) as u64;
        // `remaining * TICK_HZ / hz`, rounded up; split so the product cannot
        // overflow `u64`.
        let whole = (remaining / hz) * embassy_time_driver::TICK_HZ;
        let frac = ((remaining % hz) * embassy_time_driver::TICK_HZ).div_ceil(hz);
        self.wait_ticks(whole + frac);
    }

    /// Busy-wait for `us` microseconds (CH573 down-counter).
    ///
    /// The one-shot `STRELOAD` restarts the counter at every alarm, so there
    /// is no raw reference to chase; everything reads the driver's timebase.
    #[cfg(systick_downcounter)]
    pub fn delay_us(&mut self, us: u32) {
        let ticks = (embassy_time_driver::TICK_HZ * us as u64).div_ceil(1_000_000);
        self.wait_ticks(ticks);
    }
}

impl embedded_hal::delay::DelayNs for Delay {
    #[inline]
    fn delay_ns(&mut self, ns: u32) {
        self.delay_us(ns.div_ceil(1_000));
    }

    #[inline]
    fn delay_us(&mut self, us: u32) {
        Delay::delay_us(self, us);
    }

    #[inline]
    fn delay_ms(&mut self, ms: u32) {
        Delay::delay_ms(self, ms);
    }
}

/// Measure SysTick against the RTC's 32 kHz counter.
///
/// Returns ticks per second, or the nominal frequency if the RTC never
/// advances (it should always be running, but a hang here would brick boot).
unsafe fn calibrate() -> u32 {
    let mut samples = [0u32; CAL_ROUNDS];

    for sample in samples.iter_mut() {
        let rtc_start = rtc_count();
        let start = SYSTICK.cntl().read().cntl();

        // The RTC window is 125 ms whatever Fsys is. A stalled RTC must not
        // brick boot, so fall back rather than hang: the guard is far larger
        // than the loop can run in 125 ms at any supported clock.
        let mut guard = 0u32;
        let rtc_delta = loop {
            let delta = rtc_count().wrapping_sub(rtc_start);
            if delta >= CAL_COUNTS {
                break delta;
            }
            guard += 1;
            if guard > 200_000_000 {
                return crate::rcc::clocks().sysclk.0;
            }
            core::hint::spin_loop();
        };

        let end = SYSTICK.cntl().read().cntl();
        let ticks = elapsed_ticks(start, end) as u64;
        let rtc_delta = rtc_delta as u64;

        // ticks per second = ticks * RTC_HZ / rtc_delta
        *sample = (ticks * RTC_HZ as u64 / rtc_delta) as u32;
    }

    samples.sort_unstable();
    let hz = samples[CAL_ROUNDS / 2];
    if hz == 0 {
        crate::rcc::clocks().sysclk.0
    } else {
        hz
    }
}
