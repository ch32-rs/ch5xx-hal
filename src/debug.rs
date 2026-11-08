//! Debug output.
//!
//! CH592 exposes the WCH-Link debug module's SDI print channel: firmware
//! writes to `DATA0`/`DATA1` in the debug module and the Link forwards the
//! bytes to its USB serial port. The driver is implemented for CH592 only;
//! other dies keep a no-op (use `defmt` over SEGGER RTT there).
//!
//! # Protocol
//!
//! A message is up to seven bytes. `DATA0` holds a length byte followed by the
//! first three characters; `DATA1` holds characters four to seven. A write
//! waits until `DATA0` reads zero, meaning the Link has drained the previous
//! word. Register addresses follow `ch32fun`'s `ch5xxhw.h`: CH584/CH585 use
//! `0xE000_0340`, every other CH5xx die uses `0xE000_0380`.
//!
//! Enabling the channel is the debugger's job (`wlink flash
//! --enable-sdi-print`); firmware only writes. The status word one word past
//! `DATA1` reads zero while a debugger is attached, which gates the writes so
//! a board without a probe does not spin waiting for a drain that never comes.

/// Placeholder console.
///
/// On CH592 this writes to the WCH-Link SDI print channel; on other dies it
/// discards everything, so example code written against ch32-hal keeps
/// compiling.
pub struct SDIPrint;

#[cfg(ch592)]
mod sdi {
    /// First debug-module data word: length and characters 0-2.
    pub const DATA0: *mut u32 = 0xE000_0380 as *mut u32;
    /// Second debug-module data word: characters 3-6.
    pub const DATA1: *mut u32 = 0xE000_0384 as *mut u32;
    /// Reads zero while a debugger is attached.
    pub const SENTINEL: *const u32 = 0xE000_0388 as *const u32;
}

impl SDIPrint {
    /// Nothing to do: the channel is armed by the debugger.
    pub fn enable() {}

    /// Whether a previous word is still waiting for the Link.
    #[inline]
    pub fn is_busy() -> bool {
        #[cfg(ch592)]
        {
            unsafe { core::ptr::read_volatile(sdi::DATA0) != 0 }
        }
        #[cfg(not(ch592))]
        {
            false
        }
    }
}

#[cfg(ch592)]
impl core::fmt::Write for SDIPrint {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if unsafe { core::ptr::read_volatile(sdi::SENTINEL) } != 0 {
            // No debugger: writing would wait forever for a drain.
            return Ok(());
        }
        for chunk in s.as_bytes().chunks(7) {
            let mut data = [0u8; 8];
            data[0] = chunk.len() as u8;
            data[1..1 + chunk.len()].copy_from_slice(chunk);
            let data0 = u32::from_le_bytes(data[0..4].try_into().unwrap());
            let data1 = u32::from_le_bytes(data[4..8].try_into().unwrap());
            unsafe {
                while core::ptr::read_volatile(sdi::DATA0) != 0 {
                    core::hint::spin_loop();
                }
                core::ptr::write_volatile(sdi::DATA1, data1);
                core::ptr::write_volatile(sdi::DATA0, data0);
            }
        }
        Ok(())
    }
}

#[cfg(not(ch592))]
impl core::fmt::Write for SDIPrint {
    fn write_str(&mut self, _s: &str) -> core::fmt::Result {
        Ok(())
    }
}
