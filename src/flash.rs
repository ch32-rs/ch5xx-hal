//! Internal FlashROM: the code region and the user data region.
//!
//! The CH5xx flash is programmed through a serial command engine at
//! `0x4000_1800`, not through control registers like the CH32 parts. Every
//! erase or program is a command byte plus address bytes, followed by a word
//! or a byte at a time, with a handshake register polled for completion. The
//! sequence is the one the CH583/CH585 ROM routine uses, cross-checked against
//! `wlink`'s algorithm and probe-rs's CH5xx flash algorithm. The CH595 family
//! replaced the engine with a page-latch controller; that backend is separate.
//!
//! # Layout (datasheet table 2-2-1)
//!
//! | Range | Size | Content |
//! | --- | --- | --- |
//! | `0x0000_0000-0x0006_FFFF` | 448 KiB | user code |
//! | `0x0007_0000-0x0007_7FFF` | 32 KiB | user data |
//! | `0x0007_8000-0x0007_DFFF` | 24 KiB | bootloader, never exposed |
//! | `0x0007_E000-0x0007_EFFF` | 4 KiB | user info, tool-set only |
//! | `0x0007_F000-0x0007_FFFF` | 4 KiB | info flash, holds the MAC at `0x7F018` |
//!
//! CH570/CH572 have a 240 KiB code region and no user data region; their MAC
//! sits at `0x3F018` instead. The CH584X
//! die variant exposes 480 KiB and moves the code region to engine address
//! `0x0008_0000`; the driver detects the variant at run time from the chip ID,
//! as the ROM routine does, and refuses data-region access on it: that die
//! addresses DataFlash 0-based in the engine and erases in 4 KiB blocks, which
//! this driver does not implement.
//!
//! The data region's engine frame differs by ROM family: ISP585 (CH581/CH584/
//! CH585) and ISP573 (CH573) address `0x7_0000` directly, while ISP583
//! (CH582/CH583) and ISP592 (CH592) set the engine's `0x80000` bit. The CPU
//! address is `0x0007_0000-0x0007_7FFF` on all of them.
//!
//! CH595 has a 240 KiB code region, no user data region, 2 KiB sectors and
//! 64 KiB banks, and programs through a 256-byte page latch.
//!
//! The engine also holds the die's identity: `Flash::mac_address` returns the
//! six factory MAC bytes from InfoFlash and `Flash::unique_id` the 64-bit ID.
//! The CH595 controller has neither command, so its build omits both.
//!
//! On the serial-engine dies code reads go through the engine too. The byte
//! port answers with garbage on the code region, so [`CodeFlash::blocking_read`]
//! latches dwords through `R32_FLASH_DATA` — the path the ROM's own verify
//! routine uses. Erased code does not read as `0xFF`: every path answers a
//! placeholder that varies per die — measured `0xF3F9BDA9` on CH582/CH585/CH592
//! and `0xF5F9BDA9` on CH573 — so a blank check must use its own die's value.
//! CH595 reads are plain memory-mapped loads and erased bytes read `0xFF`.
//!
//! # Execution constraints
//!
//! The serial engine has no DMA and the core cannot fetch from flash while the
//! engine or the CH595 controller owns it, so every erase/program sequence runs
//! from RAM (the `.highcode` section, copied by `qingke-rt` when its `highcode`
//! feature is on — the `flash` feature enables it). Interrupts are masked per
//! operation, which makes every call a bounded stall.
//!
//! Callers must also stop anything else that touches flash or the bus for the
//! duration: the radio/DMA must be idle, and a program source buffer passed as
//! `&[u8]` must live in RAM (a `const`/`include_bytes!` slice lives in the
//! flash being programmed). The driver cannot check either at compile time.
//!
//! The unlock is a safe-access write (`0x57`, `0xA8`, then
//! `R8_GLOB_ROM_CFG = saved | 0xE0`). On the CH585 that register reads back as
//! `R8_RESET_STATUS`, whose top bits are constant, so a readback test always
//! claims "already unlocked" while the write port is still locked. This driver
//! never tests it and always runs the sequence, reading `saved` inside the
//! safe-access window.
//!
//! A failed erase can leave the engine busy and the debug port unresponsive;
//! on `Error::Timeout` the caller should stop using flash and reset the part
//! rather than retry.
//!
//! The engine's timing comes from the clock tree: `R8_FLASH_CFG`/`R8_FLASH_SCK`
//! are programmed by [`crate::rcc`] for the selected clock. With the reset
//! value at 78 MHz the engine silently drops erase and program commands, so a
//! custom clock setup must keep those writes.
//!
//! # Bring-up status
//!
//! Validated on CH585 (2026-11-15): the whole data-region path (erase, blank
//! check, program including a cross-page write, engine read-back), the code
//! region's 4 KiB sector erase and 64 KiB block erase, dword program and
//! engine read-back, including writes across a page boundary. Not run on
//! target: the CH584X die path, the CH582/CH583/CH592 `0x80000` data frame,
//! the CH595 page-latch backend, and the CH570/CH572 build (no board). A failed code erase stalls the AHB
//! and can take the debug port down until the flash is erased from outside, so
//! validate on regions the running image does not use, with an external-erase
//! recovery plan ready.

use core::marker::PhantomData;
use core::ptr::write_volatile;

use crate::{pac, peripherals, Peri};

#[cfg(ch595)]
mod latch;
#[cfg(not(ch595))]
mod serial;

/// Start of the user code region, an offset into the flash address space.
pub const CODE_FLASH_BASE: u32 = 0x0000_0000;
/// Size of the user code region exposed on this die.
#[cfg(any(ch570, ch572, ch595))]
pub const CODE_FLASH_SIZE: u32 = 0x0003_C000;
/// Size of the user code region exposed on this die.
#[cfg(not(any(ch570, ch572, ch595)))]
pub const CODE_FLASH_SIZE: u32 = 0x0007_0000;
/// Code-region size of the CH584X die, which ROM limits to 448 KiB while the
/// bootloader flag is set.
#[cfg(not(any(ch570, ch572, ch595)))]
const CH584X_CODE_FLASH_SIZE: u32 = 0x0007_8000;
/// Erase granularity of the code region.
///
/// The serial-engine dies erase 4 KiB per `0x20` command; the CH595 page-latch
/// controller erases 2 KiB sectors and 64 KiB banks.
#[cfg(ch595)]
pub const CODE_ERASE_SIZE: usize = 0x800;
/// Erase granularity of the code region; see [`CODE_ERASE_SIZE`].
#[cfg(not(ch595))]
pub const CODE_ERASE_SIZE: usize = 0x1000;
/// 64 KiB block erase; the region is not always a multiple.
pub const CODE_BLOCK_ERASE_SIZE: usize = 0x1_0000;
/// Program unit of the code region: 32-bit words.
pub const CODE_WRITE_SIZE: usize = 4;

/// `ROM_CFG_MAC_ADDR`: six MAC bytes followed by a two-byte checksum.
#[cfg(all(not(ch595), any(ch570, ch572)))]
const MAC_ADDR: u32 = 0x0003_F018;
/// `ROM_CFG_MAC_ADDR`; see the CH570/CH572 value above.
#[cfg(all(not(ch595), not(any(ch570, ch572))))]
const MAC_ADDR: u32 = 0x0007_F018;

/// Start of the user data region, an offset into the flash address space.
#[cfg(flashrom_data)]
pub const DATA_FLASH_BASE: u32 = 0x0007_0000;
/// Size of the user data region.
#[cfg(flashrom_data)]
pub const DATA_FLASH_SIZE: u32 = 0x8000;
/// Erase granularity of the data region: command `0x81`, 256 bytes.
#[cfg(flashrom_data)]
pub const DATA_ERASE_SIZE: usize = 0x100;
/// Program unit of the data region: the engine shifts in single bytes.
#[cfg(flashrom_data)]
pub const DATA_WRITE_SIZE: usize = 1;
/// Engine frame bit of the data region. ISP583 (CH582/CH583) and ISP592
/// (CH592) set the `0x80000` bit; ISP573/ISP585 parts address `0x7_0000`
/// directly. The CPU-side address stays `0x7_0000..0x7_7FFF` on both.
#[cfg(flashrom_data)]
#[cfg(flashrom_data_frame_80000)]
const DATA_FRAME_OFFSET: u32 = 0x0008_0000;
/// Engine frame bit of the data region; see [`DATA_FRAME_OFFSET`].
#[cfg(flashrom_data)]
#[cfg(not(flashrom_data_frame_80000))]
const DATA_FRAME_OFFSET: u32 = 0;
/// Value of an erased byte.
pub const ERASED: u8 = 0xFF;

/// Safe-access key bytes. Two writes in order open the protected write port.
const SAFE_ACCESS_KEY1: u8 = 0x57;
const SAFE_ACCESS_KEY2: u8 = 0xA8;

/// Attach failure, or a rejected request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// The offset or length is not a multiple of the unit the engine needs.
    NotAligned,
    /// The request leaves the region this handle owns.
    OutOfBounds,
    /// The engine did not report completion within the poll limit.
    Timeout,
    /// The engine kept refusing the command after the retry limit.
    Rejected,
    /// The die in the socket needs a different protocol than this build
    /// provides (the CH584X data region).
    Unsupported,
}

impl embedded_storage::nor_flash::NorFlashError for Error {
    fn kind(&self) -> embedded_storage::nor_flash::NorFlashErrorKind {
        use embedded_storage::nor_flash::NorFlashErrorKind;
        match self {
            Self::NotAligned => NorFlashErrorKind::NotAligned,
            Self::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            Self::Timeout | Self::Rejected | Self::Unsupported => NorFlashErrorKind::Other,
        }
    }
}

/// Blocking flash mode typestate.
pub enum Blocking {}
/// Async flash mode typestate; the operation itself still stalls.
pub enum Async {}

/// Configuration for a flash handle.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Status polls before an operation is declared timed out. One poll is a
    /// full command sequence, so the default is already seconds of engine
    /// time; the limit only exists so a broken engine cannot hang forever.
    pub poll_limit: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_limit: 0x0020_0000,
        }
    }
}

/// The flash controller.
pub struct Flash<'d, MODE = Blocking> {
    _peri: Peri<'d, peripherals::FLASHROM>,
    _mode: PhantomData<MODE>,
    config: Config,
}

impl<'d> Flash<'d, Blocking> {
    /// Create a flash handle.
    pub fn new(peri: Peri<'d, peripherals::FLASHROM>) -> Self {
        Self::new_with_config(peri, Config::default())
    }

    /// Create a flash handle with an explicit poll limit.
    pub fn new_with_config(peri: Peri<'d, peripherals::FLASHROM>, config: Config) -> Self {
        Self {
            _peri: peri,
            _mode: PhantomData,
            config,
        }
    }
}

impl<'d, MODE> Flash<'d, MODE> {
    /// Borrow the user code region.
    pub fn code_flash(&mut self) -> CodeFlash<'_, 'd, MODE> {
        CodeFlash { flash: self }
    }

    /// The six factory MAC bytes, read from InfoFlash through the engine.
    ///
    /// This is the address the BLE ROM library broadcasts by default. Reading
    /// requires the die's read-out protection to be off (`CFG_ROM_READ`).
    #[cfg(not(ch595))]
    pub fn mac_address(&mut self) -> Result<[u8; 6], Error> {
        let is_584x = unsafe { serial::is_584x() };
        let mut bytes = [0u8; 8];
        unsafe { serial::ram_read_words(MAC_ADDR, bytes.as_mut_ptr(), 8, is_584x)? };
        Ok([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]])
    }

    /// The 64-bit factory unique ID.
    ///
    /// WCH documents it as one ID per die; the ROM derives it from the engine's
    /// sixteen-byte answer, not from a plain memory read.
    #[cfg(not(ch595))]
    pub fn unique_id(&mut self) -> Result<u64, Error> {
        let mut id = [0u8; 8];
        unsafe { serial::ram_read_unique_id(id.as_mut_ptr())? };
        Ok(u64::from_le_bytes(id))
    }

    /// Borrow the user data region.
    #[cfg(flashrom_data)]
    pub fn data_flash(&mut self) -> DataFlash<'_, 'd, MODE> {
        DataFlash { flash: self }
    }
}

/// The user code region.
///
/// Offsets are relative to the start of the region. Programming does not erase
/// first; a word may only be programmed once between erases.
pub struct CodeFlash<'f, 'd, MODE = Blocking> {
    flash: &'f mut Flash<'d, MODE>,
}

#[cfg(not(ch595))]
impl<MODE> CodeFlash<'_, '_, MODE> {
    /// The region size in bytes on the die in the socket.
    pub fn capacity(&self) -> usize {
        serial::code_capacity(unsafe { serial::is_584x() }) as usize
    }

    /// Read the region through the command engine.
    ///
    /// Reads are 4-byte aligned and use the engine's dword latch; the byte
    /// port does not return the real contents of the code region.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
        let is_584x = unsafe { serial::is_584x() };
        check_slice(offset, bytes.len(), CODE_WRITE_SIZE, serial::code_capacity(is_584x))?;
        if bytes.is_empty() {
            return Ok(());
        }
        unsafe {
            serial::ram_read_words(
                CODE_FLASH_BASE + offset,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                is_584x,
            )
        }
    }

    /// Erase `from..to` in 4 KiB sectors; both bounds are sector aligned and
    /// inside the region.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
        let is_584x = unsafe { serial::is_584x() };
        check_range(from, to, CODE_ERASE_SIZE, serial::code_capacity(is_584x))?;
        let limit = self.flash.config.poll_limit;
        for sector in (from..to).step_by(CODE_ERASE_SIZE) {
            unsafe {
                serial::ram_erase(CODE_FLASH_BASE + sector, serial::CMD_ERASE_SECTOR, is_584x, limit)?;
            }
        }
        Ok(())
    }

    /// Erase `from..to` in 64 KiB blocks. Both bounds must be block aligned;
    /// use [`Self::blocking_erase`] for the remainder.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_erase_blocks(&mut self, from: u32, to: u32) -> Result<(), Error> {
        let is_584x = unsafe { serial::is_584x() };
        check_range(from, to, CODE_BLOCK_ERASE_SIZE, serial::code_capacity(is_584x))?;
        let limit = self.flash.config.poll_limit;
        for block in (from..to).step_by(CODE_BLOCK_ERASE_SIZE) {
            unsafe {
                serial::ram_erase(CODE_FLASH_BASE + block, serial::CMD_ERASE_BLOCK, is_584x, limit)?;
            }
        }
        Ok(())
    }

    /// Program `bytes` at `offset`, 4-byte aligned, into erased space. `bytes`
    /// must live in RAM.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        let is_584x = unsafe { serial::is_584x() };
        check_slice(offset, bytes.len(), CODE_WRITE_SIZE, serial::code_capacity(is_584x))?;
        if bytes.is_empty() {
            return Ok(());
        }
        let limit = self.flash.config.poll_limit;
        unsafe {
            serial::ram_write_words(
                CODE_FLASH_BASE + offset,
                bytes.as_ptr(),
                bytes.len() as u32,
                is_584x,
                limit,
            )
        }
    }
}

#[cfg(ch595)]
impl<MODE> CodeFlash<'_, '_, MODE> {
    /// The region size in bytes.
    pub fn capacity(&self) -> usize {
        CODE_FLASH_SIZE as usize
    }

    /// Read the region (memory-mapped; the controller is not involved).
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
        check_slice(offset, bytes.len(), CODE_WRITE_SIZE, CODE_FLASH_SIZE)?;
        unsafe { latch::read(offset, bytes.as_mut_ptr(), bytes.len() as u32) }
    }

    /// Erase `from..to` in 2 KiB sectors; both bounds are sector aligned and
    /// inside the region.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
        check_range(from, to, CODE_ERASE_SIZE, CODE_FLASH_SIZE)?;
        let limit = self.flash.config.poll_limit;
        unsafe { latch::erase(from, to, limit) }
    }

    /// Erase `from..to` in 64 KiB banks. Both bounds must be block aligned;
    /// use [`Self::blocking_erase`] for the remainder.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_erase_blocks(&mut self, from: u32, to: u32) -> Result<(), Error> {
        check_range(from, to, CODE_BLOCK_ERASE_SIZE, CODE_FLASH_SIZE)?;
        let limit = self.flash.config.poll_limit;
        unsafe { latch::erase_blocks(from, to, limit) }
    }

    /// Program `bytes` at `offset`, 4-byte aligned, into erased space. `bytes`
    /// must live in RAM.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        check_slice(offset, bytes.len(), CODE_WRITE_SIZE, CODE_FLASH_SIZE)?;
        if bytes.is_empty() {
            return Ok(());
        }
        let limit = self.flash.config.poll_limit;
        unsafe { latch::write(offset, bytes.as_ptr(), bytes.len() as u32, limit) }
    }
}

/// The user data region.
///
/// Offsets are relative to the start of the region. Programming is byte-wise
/// and does not erase first.
#[cfg(flashrom_data)]
pub struct DataFlash<'f, 'd, MODE = Blocking> {
    flash: &'f mut Flash<'d, MODE>,
}

#[cfg(flashrom_data)]
impl<MODE> DataFlash<'_, '_, MODE> {
    /// The region size in bytes.
    pub const fn capacity(&self) -> usize {
        DATA_FLASH_SIZE as usize
    }
}

#[cfg(flashrom_data)]
impl<MODE> DataFlash<'_, '_, MODE> {
    /// Read the region through the command engine.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
        check_slice(offset, bytes.len(), 1, DATA_FLASH_SIZE)?;
        if bytes.is_empty() {
            return Ok(());
        }
        serial::check_die()?;
        let addr = serial::data_frame(DATA_FLASH_BASE + offset);
        unsafe { serial::ram_read_bytes(addr, bytes.as_mut_ptr(), bytes.len() as u32) }
    }

    /// Erase `from..to` in 256-byte blocks; both bounds are block aligned.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
        check_range(from, to, DATA_ERASE_SIZE, DATA_FLASH_SIZE)?;
        serial::check_die()?;
        let limit = self.flash.config.poll_limit;
        for block in (from..to).step_by(DATA_ERASE_SIZE) {
            unsafe {
                serial::ram_erase(
                    serial::data_frame(DATA_FLASH_BASE + block),
                    serial::CMD_ERASE_DATA,
                    false,
                    limit,
                )?;
            }
        }
        Ok(())
    }

    /// Program `bytes` at `offset` into erased space. `bytes` must live in RAM.
    #[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
    #[inline(never)]
    pub fn blocking_write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        check_slice(offset, bytes.len(), DATA_WRITE_SIZE, DATA_FLASH_SIZE)?;
        if bytes.is_empty() {
            return Ok(());
        }
        serial::check_die()?;
        let limit = self.flash.config.poll_limit;
        let addr = serial::data_frame(DATA_FLASH_BASE + offset);
        unsafe { serial::ram_write_bytes(addr, bytes.as_ptr(), bytes.len() as u32, limit) }
    }
}

macro_rules! read_nor_flash_impl {
    ($region:ident, $read:literal) => {
        impl<MODE> embedded_storage::nor_flash::ErrorType for $region<'_, '_, MODE> {
            type Error = Error;
        }

        impl<MODE> embedded_storage::nor_flash::ReadNorFlash for $region<'_, '_, MODE> {
            const READ_SIZE: usize = $read;

            fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
                self.blocking_read(offset, bytes)
            }

            fn capacity(&self) -> usize {
                self.capacity()
            }
        }
    };
}

#[cfg(any(ch595, flashrom_data))]
macro_rules! nor_flash_impl {
    ($region:ident, $read:literal, $write:literal, $erase:expr) => {
        read_nor_flash_impl!($region, $read);

        impl embedded_storage::nor_flash::NorFlash for $region<'_, '_, Blocking> {
            const WRITE_SIZE: usize = $write;
            const ERASE_SIZE: usize = $erase;

            fn erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
                self.blocking_erase(from, to)
            }

            fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
                self.blocking_write(offset, bytes)
            }
        }
    };
}

// Serial code flash erases to a per-die sentinel, not `0xFF`; `NorFlash` promises
// all ones, so only the latch controller can implement it.
#[cfg(ch595)]
nor_flash_impl!(CodeFlash, 4, 4, CODE_ERASE_SIZE);
#[cfg(not(ch595))]
read_nor_flash_impl!(CodeFlash, 4);
#[cfg(flashrom_data)]
nor_flash_impl!(DataFlash, 1, 1, DATA_ERASE_SIZE);

fn check_slice(offset: u32, len: usize, align: usize, size: u32) -> Result<(), Error> {
    if offset as usize % align != 0 || len % align != 0 {
        return Err(Error::NotAligned);
    }
    let end = (offset as usize).checked_add(len).ok_or(Error::OutOfBounds)?;
    if end > size as usize {
        return Err(Error::OutOfBounds);
    }
    Ok(())
}

fn check_range(from: u32, to: u32, align: usize, size: u32) -> Result<(), Error> {
    if from > to {
        return Err(Error::OutOfBounds);
    }
    if from as usize % align != 0 || to as usize % align != 0 {
        return Err(Error::NotAligned);
    }
    if to as usize > size as usize {
        return Err(Error::OutOfBounds);
    }
    Ok(())
}

#[inline(always)]
unsafe fn w8(addr: *mut u8, value: u8) {
    unsafe { write_volatile(addr, value) }
}

#[inline(always)]
unsafe fn w32(addr: *mut u32, value: u32) {
    unsafe { write_volatile(addr, value) }
}

#[cfg(any(ch585, ch592, ch572, ch595))]
#[inline(always)]
unsafe fn feed_watchdog() {
    pac::SYS.iwdg_kr().write_value(pac::sys::regs::IwdgKr(0xAAAA))
}

/// The other dies do not model the watchdog key register in `SYS` metadata, so
/// the driver cannot feed it portably. Their operations are still bounded by
/// the busy and status poll limits.
#[cfg(not(any(ch585, ch592, ch572, ch595)))]
#[inline(always)]
unsafe fn feed_watchdog() {}

#[cfg(target_arch = "riscv32")]
#[inline(always)]
unsafe fn disable_interrupts() -> usize {
    let mstatus: usize;
    unsafe { core::arch::asm!("csrrci {}, mstatus, 8", out(reg) mstatus) };
    mstatus & 8
}

#[cfg(not(target_arch = "riscv32"))]
#[inline(always)]
unsafe fn disable_interrupts() -> usize {
    0
}

#[cfg(target_arch = "riscv32")]
#[inline(always)]
unsafe fn restore_interrupts(saved: usize) {
    if saved != 0 {
        unsafe { core::arch::asm!("csrsi mstatus, 8") };
    }
}

#[cfg(not(target_arch = "riscv32"))]
#[inline(always)]
unsafe fn restore_interrupts(_saved: usize) {}
