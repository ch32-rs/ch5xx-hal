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
//! [`Delay`] polls the SysTick counter directly and turns microseconds into
//! ticks with the calibrated rate. That is the cheapest option and it works
//! with no other stack involved.
//!
//! Under `time-driver-systick` the embassy driver owns the counter and
//! reprograms its compare for every alarm, so a raw poll would measure an
//! interval against a counter that restarts underneath it: measured on
//! hardware with an alarm every 100 ms, `delay_ms(1000)` took **97 ms**.
//! [`Delay::delay_us`] therefore chases the raw counter and switches to
//! [`embassy_time_driver::now`] — monotonic by contract — as soon as the
//! counter moves underneath it, and [`Delay::delay_ms`] reads the timebase
//! through `now()` throughout. Both are one embassy tick of quantization,
//! rounded up, instead of the exactly calibrated raw path; that is the trade
//! the feature makes. See the methods for why one `now()` poll is expensive
//! on the V3C.
//!
//! Note that an executor that never sleeps hides the problem: `CMPLR` stays
//! at its initial `MAX_PERIOD - 1`, the counter never reaches it, and raw
//! polling looks fine until the first task actually sleeps.
//!
//! The rate measurement does **not** depend on any of this: `init` is a free
//! function and runs under every feature, before the time driver takes the
//! counter over.

use crate::atomic::Ordering;
use crate::pac::SYSTICK;

/// A delay backed by the SysTick timebase.
///
/// Requires [`crate::init`] to have run, so the counter is started and its rate
/// is known.
pub struct Delay;

/// SysTick ticks per second, measured against the RTC at init.
static TICKS_PER_SEC: crate::atomic::AtomicU32 = crate::atomic::AtomicU32::new(crate::rcc::DEFAULT_FREQUENCY.0);

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
    TICKS_PER_SEC.store(calibrate(), Ordering::Relaxed);
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
    #[cfg_attr(highcode, inline(never), link_section = ".highcode")]
    pub fn delay_us(&mut self, us: u32) {
        let hz = TICKS_PER_SEC.load(Ordering::Relaxed);
        debug_assert!(hz != 0, "delay used before `ch5xx_hal::init`");

        // `us * hz / 1_000_000` in 64 bits: `us` up to ~4e6 here, `hz` ~3e7, so
        // the product needs more than 32 bits.
        let mut remaining = (us as u64 * hz as u64 / 1_000_000) as u32;

        // The counter is 32 bits, so chase it in chunks to keep the
        // wrapping comparison meaningful.
        while remaining > 0 {
            let chunk = remaining.min(u32::MAX / 2);
            remaining -= chunk;

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
/// driver, so raw polling and `now()` each carry a trade-off: see the module
/// docs. `delay_us` chases the raw counter and falls back to `now()`;
/// `delay_ms` reads the timebase through `now()` throughout. Even one `now()`
/// poll is expensive: converting counter ticks to time divides by the
/// runtime-measured rate twice, and the V3C has no M extension, so each poll
/// costs tens of microseconds.
///
/// The two `delay_us` bodies below are cfg-exclusive: the up-counter chases
/// the raw register, the CH573 down-counter has a one-shot `STRELOAD` and
/// reads the timebase throughout.
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
    /// The wait is budgeted in raw counter ticks up front — one microsecond is
    /// `Fsys / 1 MHz` ticks, rounded up — and the loop only subtracts. The
    /// budget is `u64`, so any `us` fits exactly; no `u128` is needed (a
    /// `u128` division here cost ~100 µs of CPU per call at 60 MHz, which the
    /// soft-I2C half-period path called twice per bus bit).
    ///
    /// If the counter wraps, or the time driver reprograms the compare point
    /// and reloads it, the raw reference is gone and the remainder finishes on
    /// the driver timebase in its own tick units, rounded up. The fallback
    /// costs one `now()` poll plus its quantization.
    ///
    /// # Wrap detection is not preemption-proof
    ///
    /// A reload is only noticed when a read pairs a pre-reload reference with
    /// a post-reload counter still below it. If an interrupt preempts the loop
    /// for at least one full reload period (the span to the next compare,
    /// which a tight alarm queue keeps short), the counter climbs back above
    /// the reference and the missed reload under-counts by up to one reload
    /// period. The raw chase is therefore only as interrupt-tolerant as any
    /// short spin loop; route long waits through [`Delay::delay_ms`], which
    /// is safe because it reads the monotonic timebase throughout.
    #[cfg(not(systick_downcounter))]
    #[cfg_attr(highcode, inline(never), link_section = ".highcode")]
    pub fn delay_us(&mut self, us: u32) {
        let tpu = TICKS_PER_SEC.load(Ordering::Relaxed).div_ceil(1_000_000) as u64;
        let mut remaining = us as u64 * tpu;
        let mut prev = SYSTICK.cntl().read().cntl();
        loop {
            let now = SYSTICK.cntl().read().cntl();
            if now < prev {
                // Round up: a truncated remainder of 0 would return 1 µs
                // early. `left_us` converts back to the driver's tick units.
                let left_us = remaining / tpu;
                let ticks = (left_us * embassy_time_driver::TICK_HZ).div_ceil(1_000_000);
                let deadline = embassy_time_driver::now().saturating_add(ticks);
                while embassy_time_driver::now() < deadline {
                    core::hint::spin_loop();
                }
                return;
            }
            remaining = remaining.saturating_sub(now.wrapping_sub(prev) as u64);
            prev = now;
            if remaining == 0 {
                return;
            }
        }
    }

    /// Busy-wait for `us` microseconds (CH573 down-counter).
    ///
    /// The one-shot `STRELOAD` restarts the counter at every alarm, so there
    /// is no raw reference to chase at all; everything reads the driver's
    /// monotonic timebase.
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
