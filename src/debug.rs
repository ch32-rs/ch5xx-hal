//! Debug output.
//!
//! CH5xx does not support SDI printf (WCH-Link debug-module virtual serial).
//! The CH5xx datasheets document only the SWD/TWD debug interface and no SDI
//! printf mechanism — there is no `DATA0`/`DATA1` handshake register.
//!
//! For console output use `defmt` over SEGGER RTT (via `probe-rs` or
//! `probe-rs run`), which requires no special chip support.

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
