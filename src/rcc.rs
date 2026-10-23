//! Clock configuration.
//!
//! The CH5xx has no RCC block. `SYS.CLK_CFG` (`0x4000_1008`) selects the system
//! clock source and divider, and `SYS.HFCK_PWR_CTRL` powers the oscillators.
//! This module caches the resulting frequency for the delay driver and for
//! anything else that needs it.
//!
//! # Why the frequency is measured
//!
//! The datasheet's `CLK_CFG` reset value is `0x0003` (`CLK_SYS_MOD = 00`,
//! "divided from 32 MHz"), but the boot ROM does not leave it there: on the
//! CH585 used for bring-up it reads `0x014D`, i.e. `CLK_SYS_MOD = 01`
//! (PLL-sourced) with `CLK_PLL_DIV = 13`. Turning that into a frequency means
//! modelling the PLL, so instead [`crate::delay::Delay::init`] measures the
//! real rate against the independent 32 kHz RTC counter and reports it through
//! [`set_sysclk`].
//!
//! This port does **not** reconfigure the PLL: changing it needs the
//! safe-access (SAM) handshake and the crystal tuning procedure, which is
//! separate work.
//!
//! [`DEFAULT_FREQUENCY`] is only a starting value, used if calibration fails.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::time::Hertz;

/// Nominal system frequency, used until the real rate has been measured.
/// See the module docs.
pub const DEFAULT_FREQUENCY: Hertz = Hertz(32_000_000);

static SYSCLK_HZ: AtomicU32 = AtomicU32::new(DEFAULT_FREQUENCY.0);

/// Clock frequencies, as seen by drivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clocks {
    /// System clock, which drives the core and SysTick.
    pub sysclk: Hertz,
}

/// Clock configuration.
///
/// Empty for now: only the reset default (32 MHz) is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Config;

/// The cached clock frequencies.
#[inline]
pub fn clocks() -> Clocks {
    Clocks {
        sysclk: Hertz(SYSCLK_HZ.load(Ordering::Relaxed)),
    }
}

/// Record the measured system clock. Called by [`crate::delay`]'s calibration.
pub(crate) fn set_sysclk(hz: u32) {
    SYSCLK_HZ.store(hz, Ordering::Relaxed);
}

/// Apply [`Config`] and refresh the cache.
///
/// # Safety
///
/// Must be called once, before anything reads [`clocks`] or uses [`crate::delay`].
pub(crate) unsafe fn init(_config: Config) {
    // The real frequency is measured during delay calibration; leave the
    // nominal value in place until then.
    SYSCLK_HZ.store(DEFAULT_FREQUENCY.0, Ordering::Relaxed);
}
