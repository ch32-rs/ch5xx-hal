//! Serial flash command engine.
//!
//! Every die except the CH595 family: erase and program are command bytes plus
//! address/data handshakes on `FLASHROM`, and the sequences run from RAM.

use core::ptr::read_volatile;

use super::*;

// RAM-resident: a flash fetch while the engine is busy would fault.

#[inline(always)]
unsafe fn read_ctrl() -> u8 {
    pac::FLASHROM.ctrl().read()
}

#[inline(always)]
unsafe fn write_ctrl(value: u8) {
    pac::FLASHROM.ctrl().write_value(value)
}

#[inline(always)]
unsafe fn read_data() -> u8 {
    pac::FLASHROM.data_byte().read()
}

#[inline(always)]
unsafe fn write_data(value: u8) {
    pac::FLASHROM.data_byte().write_value(value)
}

#[inline(always)]
unsafe fn read_data_word() -> u32 {
    pac::FLASHROM.data().read()
}

#[inline(always)]
unsafe fn write_data_word(value: u32) {
    pac::FLASHROM.data().write_value(value)
}

#[inline(always)]
unsafe fn engine_busy() -> bool {
    read_ctrl() & CTRL_BUSY != 0
}

/// `ROM_CFG_CHIP_ID`: low nibble `0x08` marks the CH584X die variant. This is
/// an info-flash datum, not a register.
const ROM_CFG_CHIP_ID: *const u32 = 0x0007_F024 as *const u32;
/// `CHIP_ID` value shared by the CH584/CH585 dies.
const CHIP_ID_584X: u8 = 0x92;

/// Erase one code sector (4 KiB).
pub(super) const CMD_ERASE_SECTOR: u8 = 0x20;
/// Erase one code block (64 KiB).
pub(super) const CMD_ERASE_BLOCK: u8 = 0xD8;
/// Erase one data block (256 bytes).
#[cfg(flashrom_data)]
pub(super) const CMD_ERASE_DATA: u8 = 0x81;
const CMD_PROGRAM: u8 = 0x02;
const CMD_READ: u8 = 0x0B;
/// Read the 64-bit factory unique ID; read-like, so it takes no `0x06` prefix.
const CMD_READ_UNIQUE_ID: u8 = 0x4B;
/// Prefix the engine wants before an erase or program.
const CMD_PREFIX: u8 = 0x06;
const CMD_RESET: u8 = 0xFF;
const CMD_STATUS: u8 = 0x05;
// Left unexposed: boot info (`0x06` at `0x7DFF8`, inconsistent byte count), power
// up/down (`0x0C`/`0x0D`) and CH57x's protection lock (`0x08`, can lock the part).

/// Status answer for a completed erase or program.
const STATUS_DONE: u8 = 0x00;
/// Seen on the CH585 after a chunk while the previous cycle was finishing.
/// WCH's `FLASH_ROM_WAIT` only tests bit 0 and accepts `0x02`; this driver
/// settles and re-issues because treating it as done dropped writes on-board.
const STATUS_REJECTED: u8 = 0x02;

/// Re-issues of a rejected command before it is reported as an error.
const COMMAND_ATTEMPTS: u32 = 4;
/// Settle spins between a rejection and the re-issue.
const SETTLE_SPINS: u32 = 0x2000;
/// `FLASHROM.CTRL` bit 7: the engine is busy.
const CTRL_BUSY: u8 = 0x80;
/// `FLASHROM.CTRL` value that releases the current handshake.
const CTRL_IDLE: u8 = 0x00;
/// `FLASHROM.CTRL` value that starts a command frame.
const CTRL_START: u8 = 0x05;
/// `FLASHROM.CTRL` value that restarts the command engine.
const CTRL_ENGINE_RESET: u8 = 0x04;
/// `FLASHROM.CTRL` value that latches one dword of the code program path.
const CTRL_DWORD_LATCH: u8 = 0x15;
/// Engine frame offset of the code region on the CH584X die variant.
const CH584X_CODE_FRAME_OFFSET: u32 = 0x0008_0000;

/// Poll limit. The engine answers in microseconds; this only bounds a hang.
const BUSY_SPINS: u32 = 0x0100_0000;

/// Start a command: the engine expects a zero, a five, two dead cycles, then
/// the command byte.
#[inline(always)]
unsafe fn begin(command: u8) {
    unsafe {
        write_ctrl(CTRL_IDLE);
        write_ctrl(CTRL_START);
        core::arch::asm!("nop", "nop");
        write_data(command);
    }
}

#[inline(always)]
unsafe fn wait_ready() -> Result<(), Error> {
    for _ in 0..BUSY_SPINS {
        if !unsafe { engine_busy() } {
            return Ok(());
        }
    }
    Err(Error::Timeout)
}

#[inline(always)]
unsafe fn wait_done() -> Result<(), Error> {
    unsafe {
        wait_ready()?;
        write_ctrl(CTRL_IDLE);
    }
    Ok(())
}

/// Send one data byte once the engine is ready.
#[inline(always)]
unsafe fn out(byte: u8) -> Result<(), Error> {
    unsafe {
        wait_ready()?;
        write_data(byte);
    }
    Ok(())
}

/// Receive one data byte once the engine is ready.
#[inline(always)]
unsafe fn read_byte() -> Result<u8, Error> {
    unsafe {
        wait_ready()?;
        Ok(read_data())
    }
}

/// Send the 24-bit address, high byte first.
#[inline(always)]
unsafe fn address(addr: u32) -> Result<(), Error> {
    unsafe {
        out((addr >> 16) as u8)?;
        out((addr >> 8) as u8)?;
        out(addr as u8)
    }
}

/// Start a non-read command at engine address `addr`.
#[inline(always)]
unsafe fn addr_cmd(command: u8, addr: u32) -> Result<(), Error> {
    unsafe {
        begin(CMD_PREFIX);
        wait_done()?;
        begin(command);
        address(addr)
    }
}

/// Start a read-like command; the engine expects a five-byte frame (three
/// address bytes and two zero bytes) and no `0x06` prefix.
#[inline(always)]
unsafe fn read_frame(command: u8, addr: u32) -> Result<(), Error> {
    unsafe {
        begin(command);
        address(addr)?;
        out(0)?;
        out(0)
    }
}

/// Reset the engine before erase/program. No parameter byte: with one, the
/// CH583/CH585 engine ignores the next command.
#[inline(always)]
unsafe fn engine_reset() -> Result<(), Error> {
    unsafe {
        write_ctrl(CTRL_ENGINE_RESET);
        begin(CMD_RESET);
        wait_done()
    }
}

/// Poll command 5: a leading byte, then the status. Returns `false` when the
/// command must be re-issued.
#[inline(always)]
unsafe fn poll_status(limit: u32) -> Result<bool, Error> {
    for _ in 0..limit {
        unsafe {
            wait_done()?;
            begin(CMD_STATUS);
            wait_ready()?;
            let _ = read_byte()?;
            let status = read_byte()?;
            wait_done()?;
            if status == STATUS_DONE {
                return Ok(true);
            }
            if status == STATUS_REJECTED {
                return Ok(false);
            }
        }
    }
    Err(Error::Timeout)
}

/// Wait a few microseconds for the previous program or erase cycle to finish.
#[inline(always)]
unsafe fn settle() {
    for _ in 0..SETTLE_SPINS {
        unsafe { core::hint::black_box(read_ctrl()) };
    }
}

/// Open the safe-access window and enable the control and write ports.
#[inline(always)]
unsafe fn open_safe_access() {
    use pac::system_control::regs::SafeAccessSig;
    pac::SYSTEM_CONTROL
        .safe_access_sig()
        .write_value(SafeAccessSig(SAFE_ACCESS_KEY1));
    pac::SYSTEM_CONTROL
        .safe_access_sig()
        .write_value(SafeAccessSig(SAFE_ACCESS_KEY2));
    // The readback aliases the reset status while locked, so always run the
    // writes instead of testing first.
    #[cfg(not(any(ch570, ch572)))]
    pac::SYSTEM_CONTROL.glob_rom_cfg().modify(|w| {
        w.set_rom_ctrl_en(true);
        w.set_rom_data_we(true);
        w.set_rom_code_we(true);
    });
    #[cfg(any(ch570, ch572))]
    pac::SYSTEM_CONTROL.glob_rom_cfg().modify(|w| {
        w.set_rom_ctrl_en(true);
        w.set_rom_code_we(0b11);
    });
    pac::SYSTEM_CONTROL.safe_access_sig().write_value(SafeAccessSig(0));
}

/// Restore write protection, keeping the code-offset bit as the ROM routine does.
#[inline(always)]
unsafe fn close_safe_access() {
    use pac::system_control::regs::{GlobRomCfg, SafeAccessSig};
    pac::SYSTEM_CONTROL
        .safe_access_sig()
        .write_value(SafeAccessSig(SAFE_ACCESS_KEY1));
    pac::SYSTEM_CONTROL
        .safe_access_sig()
        .write_value(SafeAccessSig(SAFE_ACCESS_KEY2));
    let saved = pac::SYSTEM_CONTROL.glob_rom_cfg().read();
    pac::SYSTEM_CONTROL
        .glob_rom_cfg()
        .write_value(GlobRomCfg(saved.0 & 0x10));
    pac::SYSTEM_CONTROL.safe_access_sig().write_value(SafeAccessSig(0));
}

/// One unlock window with interrupts masked; runs from RAM.
#[inline(always)]
unsafe fn unlocked<T>(body: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    unsafe {
        let saved = disable_interrupts();
        feed_watchdog();
        open_safe_access();

        let result = engine_reset().and_then(|()| body());

        close_safe_access();
        feed_watchdog();
        restore_interrupts(saved);
        result
    }
}

/// CH584X variant: chip ID `0x92` plus the info-flash marker. The read must
/// happen before the unlock window opens.
#[inline(always)]
pub(super) unsafe fn is_584x() -> bool {
    unsafe {
        if pac::SYSTEM_CONTROL.chip_id().read().chip_id() != CHIP_ID_584X {
            return false;
        }
        (read_volatile(ROM_CFG_CHIP_ID) & 0x0F) == 0x08
    }
}

/// Code-region ceiling of the die in the socket. The CH584X die exposes
/// 480 KiB unless the ROM's bootloader flag caps it at the usual 448 KiB.
pub(super) fn code_capacity(is_584x: bool) -> u32 {
    if !is_584x {
        return CODE_FLASH_SIZE;
    }
    #[cfg(not(any(ch570, ch572)))]
    {
        if pac::SYSTEM_CONTROL.glob_cfg_info().read().boot_loader() {
            CODE_FLASH_SIZE
        } else {
            CH584X_CODE_FLASH_SIZE
        }
    }
    #[cfg(any(ch570, ch572))]
    {
        CODE_FLASH_SIZE
    }
}

/// Engine frame address for the code region. CH584X maps CodeFlash at engine
/// `CPU + 0x8_0000`.
#[inline(always)]
pub(super) fn code_frame(addr: u32, is_584x: bool) -> u32 {
    if is_584x {
        addr | CH584X_CODE_FRAME_OFFSET
    } else {
        addr
    }
}

/// Engine address of a data-region byte; the frame bit differs by ROM family.
#[cfg(flashrom_data)]
#[inline(always)]
pub(super) fn data_frame(addr: u32) -> u32 {
    addr | DATA_FRAME_OFFSET
}

/// CH584X has no supported data path: its 0-based frame would hit CodeFlash.
#[cfg(flashrom_data)]
#[inline(always)]
pub(super) fn check_die() -> Result<(), Error> {
    if unsafe { is_584x() } {
        Err(Error::Unsupported)
    } else {
        Ok(())
    }
}

/// Erase one sector/block. Runs from RAM.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn ram_erase(addr: u32, command: u8, is_584x: bool, limit: u32) -> Result<(), Error> {
    let frame = code_frame(addr, is_584x);
    unsafe {
        unlocked(|| {
            let mut attempt = 1;
            loop {
                addr_cmd(command, frame)?;
                if poll_status(limit)? {
                    return Ok(());
                }
                if attempt == COMMAND_ATTEMPTS {
                    return Err(Error::Rejected);
                }
                attempt += 1;
                settle();
            }
        })
    }
}

/// Read `len` bytes of the data region at engine address `addr` through the
/// byte port. Runs from RAM.
#[cfg(flashrom_data)]
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn ram_read_bytes(addr: u32, buf: *mut u8, len: u32) -> Result<(), Error> {
    unsafe {
        // One unlock window for the whole read, like the ROM routine.
        unlocked(|| {
            let mut read = 0;
            while read < len {
                // The engine's read command stops at a 256-byte block boundary,
                // so every chunk starts its own command.
                let to_boundary = 0x100 - ((addr + read) & 0xFF);
                let chunk = core::cmp::min(len - read, to_boundary);
                read_frame(CMD_READ, addr + read)?;
                for index in 0..chunk {
                    *buf.add((read + index) as usize) = read_byte()?;
                }
                wait_done()?;
                feed_watchdog();
                read += chunk;
            }
            Ok(())
        })
    }
}

/// Read `len` bytes of the code region, 4-byte aligned, through
/// `FLASHROM.DATA`. Runs from RAM.
///
/// The byte port answers with garbage on the code region (measured on the
/// CH585); the ROM's own verify routine reads four byte handshakes and then
/// the dword the engine latched, which returns the real contents.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn ram_read_words(addr: u32, buf: *mut u8, len: u32, is_584x: bool) -> Result<(), Error> {
    unsafe {
        unlocked(|| {
            let mut read = 0;
            while read < len {
                let at = addr + read;
                let to_boundary = 0x100 - (at & 0xFF);
                let chunk = core::cmp::min(len - read, to_boundary);
                read_frame(CMD_READ, code_frame(at, is_584x))?;
                let mut done = 0;
                while done < chunk {
                    for _ in 0..4 {
                        read_byte()?;
                    }
                    // Fixed 4-byte copy: a variable-length one compiles into a
                    // call to flash-resident `memcpy`, which faults here.
                    let bytes = u32::from_le(read_data_word()).to_le_bytes();
                    let index = (read + done) as usize;
                    *buf.add(index) = bytes[0];
                    *buf.add(index + 1) = bytes[1];
                    *buf.add(index + 2) = bytes[2];
                    *buf.add(index + 3) = bytes[3];
                    done += 4;
                }
                wait_done()?;
                feed_watchdog();
                read += chunk;
            }
            Ok(())
        })
    }
}

/// Read the 64-bit factory unique ID. Runs from RAM.
///
/// The engine answers with sixteen bytes; the vendor library XOR-folds the
/// bytes eight apart into the eight it returns.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn ram_read_unique_id(buf: *mut u8) -> Result<(), Error> {
    unsafe {
        unlocked(|| {
            for index in 0..8 {
                *buf.add(index) = 0;
            }
            read_frame(CMD_READ_UNIQUE_ID, 0)?;
            for index in 0..16 {
                let byte = read_byte()?;
                *buf.add(index & 7) ^= byte;
            }
            wait_done()
        })
    }
}

/// Program `len` bytes byte-wise (data region). Runs from RAM.
#[cfg(flashrom_data)]
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn ram_write_bytes(addr: u32, data: *const u8, len: u32, limit: u32) -> Result<(), Error> {
    unsafe {
        // One unlock window for the whole write; the ROM routine re-sends the
        // program command at each block boundary without touching the enables.
        unlocked(|| {
            let mut sent = 0;
            while sent < len {
                let to_boundary = DATA_ERASE_SIZE as u32 - ((addr + sent) & (DATA_ERASE_SIZE as u32 - 1));
                let chunk = core::cmp::min(len - sent, to_boundary);
                let mut attempt = 1;
                loop {
                    addr_cmd(CMD_PROGRAM, addr + sent)?;
                    for index in 0..chunk {
                        out(read_volatile(data.add((sent + index) as usize)))?;
                    }
                    wait_done()?;
                    if poll_status(limit)? {
                        break;
                    }
                    if attempt == COMMAND_ATTEMPTS {
                        return Err(Error::Rejected);
                    }
                    attempt += 1;
                    settle();
                }
                feed_watchdog();
                sent += chunk;
            }
            Ok(())
        })
    }
}

/// Program `len` bytes as 32-bit words (code region). Runs from RAM.
#[cfg_attr(target_arch = "riscv32", link_section = ".highcode")]
#[inline(never)]
pub(super) unsafe fn ram_write_words(
    addr: u32,
    data: *const u8,
    len: u32,
    is_584x: bool,
    limit: u32,
) -> Result<(), Error> {
    unsafe {
        // One unlock window for the whole write, as for the byte path.
        unlocked(|| {
            let mut sent = 0;
            while sent < len {
                let to_boundary = 0x100 - ((addr + sent) & 0xFF);
                let chunk = core::cmp::min(len - sent, to_boundary);
                let mut attempt = 1;
                loop {
                    addr_cmd(CMD_PROGRAM, code_frame(addr + sent, is_584x))?;
                    let mut offset = 0;
                    while offset < chunk {
                        let p = data.add((sent + offset) as usize);
                        let word = u32::from_le_bytes([
                            read_volatile(p),
                            read_volatile(p.add(1)),
                            read_volatile(p.add(2)),
                            read_volatile(p.add(3)),
                        ]);
                        write_data_word(word);
                        // Four handshakes latch one dword.
                        for _ in 0..4 {
                            wait_ready()?;
                            write_ctrl(CTRL_DWORD_LATCH);
                        }
                        offset += CODE_WRITE_SIZE as u32;
                    }
                    wait_done()?;
                    if poll_status(limit)? {
                        break;
                    }
                    if attempt == COMMAND_ATTEMPTS {
                        return Err(Error::Rejected);
                    }
                    attempt += 1;
                    settle();
                }
                feed_watchdog();
                sent += chunk;
            }
            Ok(())
        })
    }
}
