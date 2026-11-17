//! CH595 page-latch flash controller.
//!
//! CH595 and CH596 replaced the serial command engine with a hardware state
//! machine at `0x4000_1800` that programs through a 256-byte page latch and
//! erases 2 KiB sectors or 64 KiB banks. Reads are plain memory-mapped loads;
//! the controller is not involved.
//!
//! The sequence follows the vendor `CH59x_flash.c`: turn off the safe-access
//! auto-close so the write channel stays open across the long waits, unlock
//! with the standard signature and `R32_GLOBAL_CONFIG = 0xE1`, then run
//! pre-program plus erase or latch plus program and poll `R32_FLASH_STATUS`
//! (bit 31 busy, bit 19 ready).

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

use super::*;

/// Register file of the page-latch controller.
///
/// `ch5xx-data` models the byte-wide control registers as 32-bit, so the PAC
/// accessors would emit misaligned word stores. Keep byte accesses here until
/// the data block declares their widths.
#[repr(C)]
struct Controller {
    /// `R8_PREPG_CTRL` (0x00): pre-program mode and start bit.
    prepg_ctrl: u8,
    /// `R8_BUF_CTRL` (0x01): clear and auto-increment latch load.
    buf_ctrl: u8,
    /// `R8_ERASE_CTRL` (0x02): erase mode and start bit.
    erase_ctrl: u8,
    /// `R8_PROGRAM_CTRL` (0x03): page program start.
    program_ctrl: u8,
    _reserved0: [u8; 3],
    /// `R8_READ_CTRL` (0x07): flash clock selection and auto-increment.
    read_ctrl: u8,
    _reserved1: [u8; 4],
    /// `R32_FLASH_ADDR` (0x0c): auto-increments with the latch data.
    addr: u32,
    /// `R32_FLASH_DATA` (0x10): latch data.
    data: u32,
    _reserved2: [u8; 12],
    /// `R32_FLASH_STATUS` (0x20): bit 31 busy, bit 19 ready.
    status: u32,
}

const CONTROLLER: *mut Controller = pac::FLASHROM.as_ptr() as *mut Controller;

/// `R8_GLOBAL_CONFIG` after the safe-access signature: control interface and
/// both write enables.
const GLOBAL_CONFIG_UNLOCKED: u8 = 0xE1;
/// `CH59x_flash.h` `ROM_CFG_ADDR`: touching it after an operation restores
/// flash reads, a vendor workaround.
const ROM_CFG_ADDR: *mut u32 = 0x0004_0200 as *mut u32;

const STATUS_BUSY: u32 = 1 << 31;
const STATUS_READY: u32 = 1 << 19;

const PREPG_SECTOR: u8 = 1 << 1;
const PREPG_BANK: u8 = 2 << 1;
const PREPG_START: u8 = 1 << 4;

const ERASE_SECTOR: u8 = 1 << 0;
const ERASE_BANK: u8 = 1 << 1;
const ERASE_START: u8 = 1 << 4;

const BUF_CLEAR: u8 = 1 << 0;
const BUF_LOAD_AUTO: u8 = 1 << 5;

/// `RB_FLASH_CLK = 11` (two system clocks high) plus address auto-increment.
const READ_CTRL_WRITE: u8 = (3 << 4) | (1 << 2);

/// Page-latch programming granularity.
const PAGE_SIZE: u32 = 0x100;

/// Read the region. Erased flash reads `0xFF`, unlike the serial-engine dies.
pub(super) unsafe fn read(offset: u32, buf: *mut u8, len: u32) -> Result<(), Error> {
    for index in 0..len {
        unsafe {
            *buf.add(index as usize) = read_volatile((offset + index) as *const u8);
        }
    }
    Ok(())
}

/// Erase `from..to` in 2 KiB sectors.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn erase(from: u32, to: u32, limit: u32) -> Result<(), Error> {
    unsafe {
        unlocked(|| {
            for sector in (from..to).step_by(CODE_ERASE_SIZE) {
                erase_mode(sector, PREPG_SECTOR, ERASE_SECTOR, limit)?;
                feed_watchdog();
            }
            Ok(())
        })
    }
}

/// Erase `from..to` in 64 KiB banks.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn erase_blocks(from: u32, to: u32, limit: u32) -> Result<(), Error> {
    unsafe {
        unlocked(|| {
            for bank in (from..to).step_by(CODE_BLOCK_ERASE_SIZE) {
                erase_mode(bank, PREPG_BANK, ERASE_BANK, limit)?;
                feed_watchdog();
            }
            Ok(())
        })
    }
}

/// Program `len` bytes from `data`, one latch load per page.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn write(addr: u32, data: *const u8, len: u32, limit: u32) -> Result<(), Error> {
    unsafe {
        unlocked(|| {
            let mut offset = 0;
            while offset < len {
                // The latch address only auto-increments inside the page.
                let page_remaining = PAGE_SIZE - ((addr + offset) & (PAGE_SIZE - 1));
                let chunk = core::cmp::min(len - offset, page_remaining);

                w8(addr_of_mut!((*CONTROLLER).buf_ctrl), BUF_CLEAR);
                ready(limit)?;
                w8(addr_of_mut!((*CONTROLLER).read_ctrl), READ_CTRL_WRITE);
                w8(addr_of_mut!((*CONTROLLER).buf_ctrl), BUF_LOAD_AUTO);
                w32(addr_of_mut!((*CONTROLLER).addr), addr + offset);

                // The latch takes 32-bit words; pad the tail with the erased
                // value instead of reading past the caller's buffer.
                let words = chunk.div_ceil(4);
                for word in 0..words {
                    let mut value = 0u32;
                    for byte in 0..4 {
                        let index = word * 4 + byte;
                        let value_byte = if index < chunk {
                            read_volatile(data.add((offset + index) as usize))
                        } else {
                            ERASED
                        };
                        value |= (value_byte as u32) << (byte * 8);
                    }
                    ready(limit)?;
                    w32(addr_of_mut!((*CONTROLLER).data), value);
                }

                ready(limit)?;
                w32(addr_of_mut!((*CONTROLLER).addr), addr + offset);
                w8(addr_of_mut!((*CONTROLLER).program_ctrl), 1);
                ready(limit)?;
                feed_watchdog();
                offset += chunk;
            }
            Ok(())
        })
    }
}

/// One pre-program plus erase cycle.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
unsafe fn erase_mode(addr: u32, prepg: u8, erase: u8, limit: u32) -> Result<(), Error> {
    unsafe {
        w32(addr_of_mut!((*CONTROLLER).addr), addr);
        w8(addr_of_mut!((*CONTROLLER).prepg_ctrl), prepg);
        w8(addr_of_mut!((*CONTROLLER).prepg_ctrl), prepg | PREPG_START);
        ready(limit)?;
        w8(addr_of_mut!((*CONTROLLER).erase_ctrl), erase | ERASE_START);
        let result = ready(limit);
        // Clear the start bits even on failure so the next command starts clean.
        w8(addr_of_mut!((*CONTROLLER).erase_ctrl), 0);
        result
    }
}

/// Wait until the controller is idle and ready.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
unsafe fn ready(limit: u32) -> Result<(), Error> {
    unsafe {
        let mut spins = 0;
        while read_volatile(addr_of!((*CONTROLLER).status)) & STATUS_BUSY != 0 {
            spins += 1;
            if spins >= limit {
                return Err(Error::Timeout);
            }
        }
        spins = 0;
        while read_volatile(addr_of!((*CONTROLLER).status)) & STATUS_READY == 0 {
            spins += 1;
            if spins >= limit {
                return Err(Error::Timeout);
            }
        }
        Ok(())
    }
}

/// Open the erase/program channel for one body, then close it.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(always)]
unsafe fn unlocked<T>(body: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    unsafe {
        let saved = disable_interrupts();
        feed_watchdog();
        open_safe_access();

        let result = body();

        close_safe_access();
        feed_watchdog();
        restore_interrupts(saved);
        result
    }
}

/// Open the write channel. Safe-access auto-close is turned off first: the
/// signature only opens a short window, so nothing else may run between it and
/// the `GLOBAL_CONFIG` write.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
unsafe fn open_safe_access() {
    use pac::system_control::regs::{GlobRomCfg, SafeAccessSig};

    pac::SYS.slp_wake_cfg().modify(|w| {
        w.set_acauto_enable(false);
    });
    let sig = pac::SYSTEM_CONTROL.safe_access_sig();
    sig.write_value(SafeAccessSig(SAFE_ACCESS_KEY1));
    sig.write_value(SafeAccessSig(SAFE_ACCESS_KEY2));
    pac::SYSTEM_CONTROL
        .glob_rom_cfg()
        .write_value(GlobRomCfg(GLOBAL_CONFIG_UNLOCKED));
    w8(addr_of_mut!((*CONTROLLER).read_ctrl), 0);
    w8(addr_of_mut!((*CONTROLLER).read_ctrl), 3 << 4);
}

/// Restore auto-close and the write protection.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
unsafe fn close_safe_access() {
    pac::SYS.slp_wake_cfg().modify(|w| {
        w.set_acauto_enable(true);
    });
    pac::SYSTEM_CONTROL
        .safe_access_sig()
        .write_value(pac::system_control::regs::SafeAccessSig(0));
    // The vendor's "operate flash once to restore it" access.
    unsafe { write_volatile(ROM_CFG_ADDR, 0) };
}
