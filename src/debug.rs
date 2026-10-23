//! Debug output.
//!
//! ch32-hal's [`SDIPrint`] writes through the debug module's `DATA0`/`DATA1`
//! registers (`qingke::dm`). The QingKe V3C leaf deliberately defines no
//! `dm_dataaddr_*` capability — neither the CH585 nor the CH570 EVT ships an
//! SDI printf, so the debug module data address is unknown — and `qingke::dm`
//! does not exist on this core. There is consequently no SDI console here.
//!
//! For a serial console use a UART; the CH585 UART driver is not ported yet.
//!
//! [`SDIPrint`]: https://docs.rs/ch32-hal

/// Placeholder console that discards everything.
///
/// Present so example code written against ch32-hal keeps compiling; it emits
/// nothing on CH5xx.
pub struct SDIPrint;

impl SDIPrint {
    /// No-op on CH5xx (see the module docs).
    pub fn enable() {}

    /// Always `false`: there is nothing to drain.
    #[inline]
    pub fn is_busy() -> bool {
        false
    }
}

impl core::fmt::Write for SDIPrint {
    fn write_str(&mut self, _s: &str) -> core::fmt::Result {
        Ok(())
    }
}
