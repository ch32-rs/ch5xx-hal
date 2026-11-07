//! Window watchdog, reset cause, and battery detector.
//!
//! The window watchdog counts up at `Fsys / 131072` and overflows from 255. A
//! reload of 0 is the longest interval, about 6.3 s at the 5.333 MHz reset
//! clock and about 0.43 s at 78 MHz. Overflow can raise `WDOG_BAT` or reset the
//! chip. That interrupt is shared with the battery detector, and this driver
//! does not claim the handler; poll [`Watchdog::overflowed`] instead.
//!
//! [`reset_cause`] reads the flag the hardware latches, on the chips whose
//! system-control block has that register. CH573 does not, so the function is
//! absent there. A software reset is [`software_reset`], which does not return.
//!
//! The battery detector compares the supply with a fixed threshold and reports
//! low and lower. It does not measure a voltage; the ADC battery channel does
//! that. The control registers are safe-access.
//!
//! `WDOG_COUNT` and `RST_WDOG_CTRL` have the same fields on every selectable
//! chip except CH564 and CH569, which are not selectable. The battery detector
//! fields differ, so [`Battery`] compiles for the CH585 layout only. Only CH585
//! has been run on a board.

use core::marker::PhantomData;

use embassy_hal_internal::PeripheralType;

use crate::{pac, peripherals, rcc, Peri};

/// What the hardware recorded as the most recent reset.
#[cfg(reset_status)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetCause {
    /// `SOFTWARE_RESET` was written.
    Software,
    /// Power was applied.
    PowerOn,
    /// The window watchdog overflowed with reset enabled.
    Watchdog,
    /// The external reset pin was driven low.
    Pin,
    /// Wake from shutdown.
    ShutdownWake,
    /// A reset that does not match a flag this driver names.
    Other(u8),
}

/// Read the latched reset flag.
#[cfg(reset_status)]
pub fn reset_cause() -> ResetCause {
    match pac::SYSTEM_CONTROL.reset_status().read().reset_flag() {
        0 => ResetCause::Software,
        1 => ResetCause::PowerOn,
        2 => ResetCause::Watchdog,
        3 => ResetCause::Pin,
        5 => ResetCause::ShutdownWake,
        other => ResetCause::Other(other),
    }
}

/// Reset the chip. Does not return.
pub fn software_reset() -> ! {
    // `WDOG_INT_FLAG` is write-1-to-clear, so the written flag bit stays 0.
    rcc::with_safe_access(|| {
        pac::SYSTEM_CONTROL.rst_wdog_ctrl().modify(|w| {
            w.set_software_reset(true);
            w.set_wdog_int_flag(false);
        });
    });
    loop {
        core::hint::spin_loop();
    }
}

/// The window watchdog.
pub struct Watchdog<'d, T: Instance> {
    _phantom: PhantomData<&'d mut T>,
}

impl<'d, T: Instance> Watchdog<'d, T> {
    /// Take ownership of the counter.
    ///
    /// Reset is left disabled, so an overflow does nothing until
    /// [`Self::enable_reset`] is called. `reload` is the count the counter
    /// starts from; it counts up to 255.
    pub fn new(_peri: Peri<'d, T>, reload: u8) -> Self {
        let watchdog = Self { _phantom: PhantomData };
        watchdog.feed(reload);
        watchdog
    }

    /// Write `reload` into the counter, which also clears the overflow flag.
    pub fn feed(&self, reload: u8) {
        pac::SYSTEM_CONTROL.wdog_count().write(|w| w.set_wdog_count(reload));
    }

    /// Let an overflow reset the chip.
    pub fn enable_reset(&self) {
        self.set_reset_enabled(true);
    }

    /// Stop an overflow from resetting the chip.
    pub fn disable_reset(&self) {
        self.set_reset_enabled(false);
    }

    fn set_reset_enabled(&self, enabled: bool) {
        // `WDOG_INT_FLAG` is write-1-to-clear. A read-modify-write that copies a
        // set flag back would clear it, so the written flag bit stays 0.
        rcc::with_safe_access(|| {
            pac::SYSTEM_CONTROL.rst_wdog_ctrl().modify(|w| {
                w.set_wdog_rst_en(enabled);
                w.set_wdog_int_flag(false);
            });
        });
    }

    /// Whether the counter has overflowed since the last feed or clear.
    pub fn overflowed(&self) -> bool {
        pac::SYSTEM_CONTROL.rst_wdog_ctrl().read().wdog_int_flag()
    }

    /// Clear the overflow flag without reloading the counter.
    pub fn clear_overflow(&self) {
        // Write the whole byte. A read-modify-write would copy `SOFTWARE_RESET`
        // back, and that bit resets the chip.
        let mut ctrl = pac::SYSTEM_CONTROL.rst_wdog_ctrl().read();
        ctrl.set_software_reset(false);
        ctrl.set_wdog_int_flag(true);
        rcc::with_safe_access(|| {
            pac::SYSTEM_CONTROL.rst_wdog_ctrl().write_value(ctrl);
        });
    }
}

// --- battery detector ----------------------------------------------------------

/// Threshold of the high-accuracy detector.
#[cfg(battery_ch585)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Level {
    /// About 1.9 V.
    V1_9 = 0,
    /// About 2.1 V.
    V2_1 = 1,
    /// About 2.3 V.
    V2_3 = 2,
    /// About 2.5 V.
    V2_5 = 3,
}

/// What the detector reports.
#[cfg(battery_ch585)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Status {
    /// Below the selected threshold.
    pub low: bool,
    /// Below the lower threshold.
    pub lower: bool,
}

/// The supply-voltage detector.
#[cfg(battery_ch585)]
pub struct Battery<'d> {
    _phantom: PhantomData<&'d mut peripherals::SYS>,
}

#[cfg(battery_ch585)]
impl<'d> Battery<'d> {
    /// Enable the high-accuracy detector at `level` and leave the interrupts off.
    ///
    /// The detector shares `WDOG_BAT` with the window watchdog, so the interrupt
    /// enables stay clear and the status is polled.
    pub fn new(_peri: Peri<'d, peripherals::SYS>, level: Level) -> Self {
        rcc::with_safe_access(|| {
            pac::SYS.bat_det_cfg().write(|w| w.set_bat_low_vth(level as u8));
            pac::SYS.bat_det_ctrl().write(|w| {
                w.set_bat_det_en(true);
                w.set_bat_low_vthx(false);
                w.set_bat_lower_ie(false);
                w.set_bat_low_ie(false);
            });
        });
        Self { _phantom: PhantomData }
    }

    /// The current detector status.
    pub fn status(&self) -> Status {
        let status = pac::SYS.bat_status().read();
        Status {
            low: status.bat_stat_low(),
            lower: status.bat_stat_lower(),
        }
    }
}

#[cfg(battery_ch585)]
impl<'d> Drop for Battery<'d> {
    fn drop(&mut self) {
        rcc::with_safe_access(|| {
            pac::SYS.bat_det_ctrl().write(|w| {
                w.set_bat_det_en(false);
                w.set_bat_low_vthx(false);
                w.set_bat_lower_ie(false);
                w.set_bat_low_ie(false);
            });
        });
    }
}

// --- instance ------------------------------------------------------------------

mod sealed {
    pub trait Instance {}
}

/// The window watchdog.
#[allow(private_bounds)]
pub trait Instance: sealed::Instance + PeripheralType {}

impl sealed::Instance for peripherals::SYSTEM_CONTROL {}

impl Instance for peripherals::SYSTEM_CONTROL {}
