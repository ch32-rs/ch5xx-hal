//! Window watchdog, reset cause, and battery detector.
//!
//! The watchdog counts up at `Fsys / 131072` (`Fsys / 524288` on CH569) and
//! overflows from 255. The overflow interrupt is shared with the battery
//! detector, so poll [`Watchdog::overflowed`] rather than claim it.
//!
//! `WDOG_COUNT` is the same on every selectable chip. `RST_WDOG_CTRL` differs
//! on CH569: the overflow flag sits at bit 3 and [7:6] is a write key. The
//! battery detector fields are the CH585 ones, shared with CH581 and CH592.

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
    // CH569 calls the byte `RST_BOOT_STAT`; the flag encoding is the same.
    #[cfg(ch569)]
    let flag = pac::SYSTEM_CONTROL.rst_boot_stat().read().reset_flag();
    #[cfg(not(ch569))]
    let flag = pac::SYSTEM_CONTROL.reset_status().read().reset_flag();
    match flag {
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
    rcc::with_safe_access(|| {
        pac::SYSTEM_CONTROL.rst_wdog_ctrl().modify(|w| {
            w.set_software_reset(true);
            // `WDOG_INT_FLAG` is write-1-to-clear: write 0 to leave it alone.
            w.set_wdog_int_flag(false);
            // CH569 drops a write without the [7:6] key.
            #[cfg(ch569)]
            {
                w.0 |= 0x40;
            }
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
        rcc::with_safe_access(|| {
            pac::SYSTEM_CONTROL.rst_wdog_ctrl().modify(|w| {
                w.set_wdog_rst_en(enabled);
                w.set_wdog_int_flag(false);
                // CH569 drops a write without the [7:6] key.
                #[cfg(ch569)]
                {
                    w.0 |= 0x40;
                }
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
        // CH569 drops a write without the [7:6] key.
        #[cfg(ch569)]
        {
            ctrl.0 |= 0x40;
        }
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
