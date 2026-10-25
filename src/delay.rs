//! Busy-wait delays on the core SysTick counter.
//!
//! CH5xx's SysTick is the WCH variant: `STK_CTLR` (`STE` bit 0, `STCLK` bit 2,
//! `MODE` bit 4), `STK_SR.CNTIF`, and the 32-bit `STK_CNTL` / `STK_CMPLR` pair.
//! There is no free-running 64-bit counter.
//!
//! The counter is started **once** by [`init`] and left running; each delay
//! only reads `STK_CNT` and compares it against a target.
//!
//! # Why the rate is measured, not assumed
//!
//! `CLK_CFG` is readable and, on this board, holds `0x0003` (HSI 16 MHz / 3),
//! so a nominal 5.333 MHz could simply be derived from it. It is measured
//! anyway because the RC oscillators are not exact and a wrong constant makes
//! every delay wrong by the same factor.
//!
//! [`init`] calibrates the counter against the 32 kHz RTC counter, the same
//! trick WCH's `OSC_CAL` uses. The result backs [`crate::rcc::clocks`], but it
//! is a measurement: it varies slightly between resets because the internal
//! 32 kHz RC is the reference.
//!
//! Note that `probe-rs read` must not be used to check this: it halts the core
//! and reports values the running firmware does not see.
//!
//! # This driver and the SysTick time driver are mutually exclusive
//!
//! With `STRE = 1` the counter reloads at `STK_CMPLR`, and the embassy SysTick
//! time driver reprograms that register to whatever its next alarm needs. A
//! [`Delay`] running at the same time then measures its interval against a
//! counter that restarts underneath it. Measured on hardware with an alarm
//! every 100 ms: `delay_ms(1000)` completed in **97 ms**, not 1000 ms. It
//! silently returns early — no hang, no error.
//!
//! It is easy to miss because an idle executor leaves `STK_CMPLR` at its
//! initial `MAX_PERIOD - 1`, so the counter never reaches it and `delay` looks
//! correct until the first task actually sleeps. To make the conflict a
//! build-time error rather than a silent wrong answer, [`Delay`] is compiled
//! out under `time-driver-systick`.
//!
//! The rate measurement does **not** depend on this module: [`crate::rcc`]
//! calibrates against the RTC in [`crate::init`] whether or not `Delay` exists.

use crate::atomic::Ordering;
use crate::pac::SYSTICK;

/// A delay backed by the SysTick counter.
///
/// Requires [`crate::init`] (or [`init`]) to have run, so the counter is
/// started and its rate is known.
///
/// Absent when the `time-driver-systick` feature is on — see the module docs.
#[cfg(not(feature = "time-driver-systick"))]
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
    // calibration window is only 31 ms, so the low half alone is enough —
    // `wrapping_sub` handles the one wrap that could occur.
    unsafe { core::ptr::read_volatile((RTC_BASE + 0x38) as *const u16) }
}

/// Start SysTick as a free-running counter and measure its rate.
///
/// This runs on every build, even when the SysTick time driver owns the
/// counter: it is what fills in [`crate::rcc::clocks`], and the driver reads
/// the frequency from there. When the driver is compiled in, its `init` runs
/// directly after this and reprograms the counter for its own reload model.
///
/// # Safety
///
/// Call once, from [`crate::init`].
pub(crate) unsafe fn init() {
    // Free-running, up-count, HCLK source (`STCLK` set — clearing it would
    // select HCLK/8). `STE` starts it.
    SYSTICK.stk_sr().modify(|w| w.set_cntif(false));
    SYSTICK.stk_cntl().write(|w| w.set_cntl(0));
    SYSTICK.stk_cnth().write(|w| w.set_cnth(0));
    SYSTICK.stk_ctlr().modify(|w| {
        w.set_mode(false); // up-count
        w.set_stclk(true); // source: HCLK (not HCLK/8)
        w.set_stie(false); // poll, never interrupt
        w.set_ste(true);
    });

    let hz = unsafe { calibrate() };
    TICKS_PER_SEC.store(hz, Ordering::Relaxed);
    crate::rcc::set_sysclk(hz);
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

#[cfg(not(feature = "time-driver-systick"))]
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

        // Bounded: at ~5.33 MHz this window is ~125 ms of spinning. A stalled
        // RTC must not brick boot, so fall back rather than hang.
        let mut guard = 0u32;
        while rtc_count().wrapping_sub(rtc_start) < CAL_COUNTS {
            guard += 1;
            if guard > 200_000_000 {
                return crate::rcc::DEFAULT_FREQUENCY.0;
            }
            core::hint::spin_loop();
        }

        let stk_end = SYSTICK.stk_cntl().read().cntl();
        let ticks = stk_end.wrapping_sub(stk_start) as u64;
        let rtc_delta = CAL_COUNTS as u64;

        // ticks per second = ticks * RTC_HZ / rtc_delta
        *sample = (ticks * RTC_HZ as u64 / rtc_delta) as u32;
    }

    samples.sort_unstable();
    let hz = samples[CAL_ROUNDS / 2];
    if hz == 0 {
        crate::rcc::DEFAULT_FREQUENCY.0
    } else {
        hz
    }
}
