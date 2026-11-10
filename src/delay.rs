//! Busy-wait delays on the core SysTick counter.
//!
//! CH5xx's SysTick is the WCH variant: `STK_CTLR` (`STE` bit 0, `STCLK` bit 2,
//! `MODE` bit 4), `STK_SR.CNTIF`, and the 32-bit `STK_CNTL` / `STK_CMPLR` pair.
//! There is no free-running 64-bit counter.
//!
//! The counter is started **once** by `init` and left running; each delay
//! only reads `STK_CNT` and compares it against a target.
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
//! Under `time-driver-systick` the embassy driver owns the counter and reloads
//! it at a compare value it reprograms for every alarm, so a raw poll would
//! measure an interval against a counter that restarts underneath it: measured
//! on hardware with an alarm every 100 ms, `delay_ms(1000)` took **97 ms**.
//! In that configuration [`Delay`] therefore reads the timebase through
//! [`embassy_time_driver::now`] instead, which is monotonic by contract. The
//! delay is no longer exactly calibrated — it is one embassy tick, rounded up —
//! but it is correct, and that is the trade the feature makes.
//!
//! Note that an executor that never sleeps hides the problem: `STK_CMPLR` stays
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
/// the skew between the `CNT_32K` and `STK_CNTL` reads: at 1024 counts the
/// same board measured anywhere from 5.33 to 7.43 MHz run to run, which then
/// scaled every delay (and the whole embassy timebase) by up to 40%.
const CAL_COUNTS: u16 = 4096;

/// Calibration rounds; the median is used so a single disturbed window cannot
/// skew the result.
const CAL_ROUNDS: usize = 5;

/// RTC frequency, for turning counts into seconds.
const RTC_HZ: u32 = 32_768;

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
    // Free-running, up-count, HCLK source (`STCLK` set — clearing it would
    // select HCLK/8). `STE` starts it. CH573 keeps the flag in `STK_CNTFG`
    // and has no counter-direction bit.
    #[cfg(systick_cntfg)]
    SYSTICK.stk_cntfg().modify(|w| w.set_cntif(false));
    #[cfg(not(systick_cntfg))]
    SYSTICK.stk_sr().modify(|w| w.set_cntif(false));
    SYSTICK.stk_cntl().write(|w| w.set_cntl(0));
    // CH572 has a single 32-bit counter register and no high half.
    #[cfg(systick_cnth)]
    SYSTICK.stk_cnth().write(|w| w.set_cnth(0));
    SYSTICK.stk_ctlr().modify(|w| {
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

            let start = SYSTICK.stk_cntl().read().cntl();
            while SYSTICK.stk_cntl().read().cntl().wrapping_sub(start) < chunk {
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

/// Under the SysTick time driver the counter is reloaded at the driver's
/// compare value, so it cannot be polled. Read the driver's timebase instead.
///
/// It is monotonic, so it carries the delay forward correctly; it just costs
/// one tick of resolution instead of a raw counter read.
#[cfg(feature = "time-driver-systick")]
impl Delay {
    /// Busy-wait for `us` microseconds.
    pub fn delay_us(&mut self, us: u32) {
        let ticks = embassy_time_driver::TICK_HZ as u128 * us as u128 / 1_000_000;
        self.wait_ticks(ticks as u64);
    }

    /// Busy-wait for `ms` milliseconds.
    pub fn delay_ms(&mut self, ms: u32) {
        let ticks = embassy_time_driver::TICK_HZ as u128 * ms as u128 / 1_000;
        self.wait_ticks(ticks as u64);
    }

    fn wait_ticks(&mut self, ticks: u64) {
        let deadline = embassy_time_driver::now().saturating_add(ticks);
        while embassy_time_driver::now() < deadline {
            core::hint::spin_loop();
        }
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
        let stk_start = SYSTICK.stk_cntl().read().cntl();

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

        let stk_end = SYSTICK.stk_cntl().read().cntl();
        let ticks = stk_end.wrapping_sub(stk_start) as u64;
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
