//! Clock configuration.
//!
//! The CH5xx has no RCC block. `SYS.CLK_CFG` (`0x4000_1008`) selects the system
//! clock source and divider, and `SYS.HFCK_PWR_CTRL` powers the oscillators.
//! This module caches the resulting frequency for the delay driver and for
//! anything else that needs it.
//!
//! # Where the frequency comes from
//!
//! On the CH585 used for bring-up the firmware reads `CLK_CFG = 0x0003`
//! (`CLK_SYS_MOD = 00`, `CLK_PLL_DIV = 3`, `OSC32M_SEL = 0`), the datasheet's
//! reset value: **HSI 16 MHz / 3 = 5.333 MHz**. The boot ROM powers the PLL but
//! does not route the system clock through it.
//!
//! Do not read this register with `probe-rs read`: that halts the core and the
//! value it reports is not what the running firmware sees (a 16-bit read of
//! `0x4000_1008` returns `0x014D`, with the neighbouring `HFCK_PWR_CTRL` byte
//! mixed in). Read it from firmware, e.g. `examples/ch585/src/bin/clk_probe.rs`.
//!
//! [`crate::delay::Delay::init`] still measures the rate against the RTC's
//! 32 kHz counter and reports it through [`set_sysclk`]. That measurement is
//! useful (the internal 32 kHz RC is a real reference) but it is **not**
//! bit-identical across resets, so it is a calibration result, not a constant.
//!
//! This port does **not** reconfigure the system clock yet: that needs the
//! safe-access (SAM) handshake and, for HSE/PLL presets, the Flash access
//! timing, which are separate work.
//!
//! [`DEFAULT_FREQUENCY`] is only a starting value, used if calibration fails.

use crate::atomic::{AtomicU32, Ordering};
use crate::time::Hertz;

/// Starting value for the cache, used until calibration runs.
///
/// This is the CH585 reset default: HSI 16 MHz divided by 3. See the module
/// docs for why it is measured rather than trusted.
pub const DEFAULT_FREQUENCY: Hertz = Hertz(5_333_333);

static SYSCLK_HZ: AtomicU32 = AtomicU32::new(DEFAULT_FREQUENCY.0);

/// Clock frequencies, as seen by drivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clocks {
    /// System clock, which drives the core and SysTick.
    pub sysclk: Hertz,
}

/// Clock configuration.
///
/// Empty for now: the boot ROM's default (HSI/3) is used as-is.
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
