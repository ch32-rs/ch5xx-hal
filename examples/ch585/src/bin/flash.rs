//! Erase, program and read back the internal flash through the HAL driver.
//!
//! Only ranges the running image does not use are touched. The data region
//! gets an erase, a blank check, three program patterns (one crossing a
//! 256-byte block boundary) and a read-back; the code region gets a 4 KiB
//! sector erase, a dword program with a cross-page tail, a 64 KiB block erase
//! and read-backs. The image ends around `0x2500`; the sectors used here start
//! at `0x3000`.
//!
//! Erased code flash does not read as `0xFF`: every path answers with a
//! per-die placeholder (`0xF3F9BDA9` here; CH573 reads `0xF5F9BDA9`).
//! The data region reads `0xFF`.
//!
//! Build with the `flash` feature:
//!
//! ```console
//! cargo run --release --bin flash --features flash
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::flash::{Blocking, Error, Flash};
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

/// Erase granularity of the data region.
const DATA_BLOCK: u32 = 0x100;
/// The two data blocks the example works in.
const DATA_REGION: usize = 2 * DATA_BLOCK as usize;
/// Erase granularity of the code region.
const SECTOR_LEN: u32 = 0x1000;
/// The 64 KiB code block erased with the block command, clear of the image.
const BLOCK: u32 = 0x1_0000;
const BLOCK_LEN: u32 = 0x1_0000;
/// What the engine answers for an erased code word on CH585 (CH573 differs).
const BLANK_WORD: [u8; 4] = [0xA9, 0xBD, 0xF9, 0xF3];

#[entry]
fn main() -> ! {
    let p = hal::init(hal::rcc::Config::HSE_PLL_78MHZ.into());
    let mut flash = Flash::new(p.FLASHROM);

    defmt::info!(
        "flash: code {} bytes, data {} bytes, sysclk {} Hz",
        flash.code_flash().capacity(),
        flash.data_flash().capacity(),
        hal::rcc::clocks().sysclk.0
    );
    match flash.mac_address() {
        Ok(mac) => defmt::info!("factory mac: {=[u8]:02x}", mac),
        Err(err) => defmt::error!("FAIL mac: {}", step("mac", err)),
    }
    match flash.unique_id() {
        Ok(id) => defmt::info!("unique id: {:#018x}", id),
        Err(err) => defmt::error!("FAIL unique id: {}", step("uid", err)),
    }

    if let Err(what) = data_round(&mut flash) {
        defmt::error!("FAIL data region: {}", what);
        halt();
    }
    if let Err(what) = code_round(&mut flash) {
        defmt::error!("FAIL code region: {}", what);
        halt();
    }
    block_round(&mut flash);

    defmt::info!("PASS flash");
    halt()
}

/// Erase two data blocks, check they are blank, program three patterns and
/// read them back.
fn data_round(flash: &mut Flash<'_, Blocking>) -> Result<(), &'static str> {
    let mut data = flash.data_flash();
    data.blocking_erase(0, DATA_REGION as u32)
        .map_err(|err| step("data erase", err))?;

    let mut blank = [0u8; DATA_REGION];
    data.blocking_read(0, &mut blank)
        .map_err(|err| step("data blank read", err))?;
    if let Some(index) = blank.iter().position(|byte| *byte != 0xFF) {
        defmt::error!("blank check failed: byte {} reads {}", index, blank[index]);
        return Err("data blank check");
    }
    defmt::info!("data erase ok, both blocks blank");

    // Plain pattern, entirely inside block 0.
    let mut first = [0u8; 100];
    for (index, byte) in first.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(0x31);
    }
    data.blocking_write(0, &first)
        .map_err(|err| step("data write 1", err))?;

    // Starts in block 0 and ends in block 1, so the program command must be
    // re-sent at the 0x100 boundary.
    let mut second = [0u8; 100];
    for (index, byte) in second.iter_mut().enumerate() {
        *byte = 0xA0 ^ (index as u8).wrapping_mul(3);
    }
    data.blocking_write(0xA0, &second)
        .map_err(|err| step("data write 2", err))?;

    // Unaligned inside block 1.
    let mut third = [0u8; 64];
    for (index, byte) in third.iter_mut().enumerate() {
        *byte = 0xFF - index as u8;
    }
    data.blocking_write(0x120, &third)
        .map_err(|err| step("data write 3", err))?;
    defmt::info!("data program ok");

    let mut readback = [0u8; 100];
    data.blocking_read(0, &mut readback)
        .map_err(|err| step("data read-back 1", err))?;
    if readback != first {
        report("read-back 1", &readback, &first);
        return Err("data read-back 1");
    }
    let mut second_readback = [0u8; 100];
    data.blocking_read(0xA0, &mut second_readback)
        .map_err(|err| step("data read-back 2", err))?;
    if second_readback != second {
        report("read-back 2", &second_readback, &second);
        return Err("data read-back 2 (block boundary)");
    }
    let mut third_readback = [0u8; 64];
    data.blocking_read(0x120, &mut third_readback)
        .map_err(|err| step("data read-back 3", err))?;
    if third_readback != third {
        report("read-back 3", &third_readback, &third);
        return Err("data read-back 3");
    }
    defmt::info!("data read-back ok, including the block-crossing write");

    // Leave the blocks blank for the next run.
    data.blocking_erase(0, DATA_REGION as u32)
        .map_err(|err| step("data final erase", err))
}

/// Erase a code sector, check it is blank, program a pattern and a
/// cross-page tail, then read both back.
fn code_round(flash: &mut Flash<'_, Blocking>) -> Result<(), &'static str> {
    let mut code = flash.code_flash();
    let sector = 0x3_000;

    let mut buf = [0u8; 256];
    code.blocking_read(sector + 0x100, &mut buf)
        .map_err(|err| step("code read before", err))?;
    defmt::info!("{:#x} before @0x100: {=[u8]:02x}", sector, &buf[..8]);

    code.blocking_erase(sector, sector + SECTOR_LEN)
        .map_err(|err| step("code erase", err))?;
    for page in 0..16u32 {
        code.blocking_read(sector + page * 0x100, &mut buf)
            .map_err(|err| step("code page read", err))?;
        let left = buf.chunks_exact(4).filter(|word| *word != BLANK_WORD).count();
        if left != 0 {
            defmt::error!("{:#x} page {} left {} words", sector, page, left);
            return Err("code blank check");
        }
    }
    defmt::info!("{:#x} sector erase ok, all pages blank", sector);

    let mut pattern = [0u8; 64];
    for (index, byte) in pattern.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(9).wrapping_add(0x11);
    }
    code.blocking_write(sector + 0x100, &pattern)
        .map_err(|err| step("code write", err))?;

    // Second write in the same sector, on the far side of a page boundary.
    let mut tail = [0u8; 16];
    for (index, byte) in tail.iter_mut().enumerate() {
        *byte = 0xA0u8.wrapping_add((index as u8).wrapping_mul(3));
    }
    code.blocking_write(sector + 0x1F8, &tail)
        .map_err(|err| step("code cross-page write", err))?;

    let mut got = [0u8; 64];
    code.blocking_read(sector + 0x100, &mut got)
        .map_err(|err| step("code read-back", err))?;
    if got != pattern {
        report("code read-back", &got, &pattern);
        return Err("code read-back");
    }
    let mut tail_got = [0u8; 16];
    code.blocking_read(sector + 0x1F8, &mut tail_got)
        .map_err(|err| step("code cross-page read-back", err))?;
    if tail_got != tail {
        report("code cross-page read-back", &tail_got, &tail);
        return Err("code cross-page read-back");
    }
    defmt::info!("{:#x} program and cross-page read-back ok", sector);
    Ok(())
}

/// Erase a whole 64 KiB block, check every word is blank, then program and
/// read back 64 bytes inside it.
fn block_round(flash: &mut Flash<'_, Blocking>) {
    let mut code = flash.code_flash();
    let mut buf = [0u8; 256];

    if let Err(err) = code.blocking_read(BLOCK + 0x100, &mut buf) {
        fail("block before read", err);
    }
    defmt::info!("{:#x} block before @0x100: {=[u8]:02x}", BLOCK, &buf[..8]);

    if let Err(err) = code.blocking_erase_blocks(BLOCK, BLOCK + BLOCK_LEN) {
        fail("block erase", err);
    }
    let mut programmed = 0u32;
    for page in 0..BLOCK_LEN / 0x100 {
        if let Err(err) = code.blocking_read(BLOCK + page * 0x100, &mut buf) {
            fail("block page read", err);
        }
        programmed += buf.chunks_exact(4).filter(|word| *word != BLANK_WORD).count() as u32;
    }
    defmt::info!("{:#x} block erase left {} non-blank words", BLOCK, programmed);

    let mut want = [0u8; 64];
    for (index, byte) in want.iter_mut().enumerate() {
        *byte = 0x71u8.wrapping_add((index as u8).wrapping_mul(0x1F));
    }
    if let Err(err) = code.blocking_write(BLOCK + 0x200, &want) {
        fail("block write", err);
    }
    if let Err(err) = code.blocking_read(BLOCK + 0x200, &mut buf[..64]) {
        fail("block read back", err);
    }
    if buf[..64] == want {
        defmt::info!("{:#x} block program read back", BLOCK);
    } else {
        report("block read-back", &buf[..64], &want);
    }
}

fn report(what: &'static str, got: &[u8], want: &[u8]) {
    let at = got
        .iter()
        .zip(want.iter())
        .position(|(got, want)| got != want)
        .unwrap_or(got.len());
    defmt::error!("{}: first mismatch at {}", what, at);
    defmt::error!("{}: got  {=[u8]:02x}", what, &got[..16.min(got.len())]);
    defmt::error!("{}: want {=[u8]:02x}", what, &want[..16.min(want.len())]);
}

fn fail(what: &'static str, err: Error) -> ! {
    defmt::error!("FAIL {}: {}", what, step(what, err));
    halt()
}

fn step(at: &'static str, err: Error) -> &'static str {
    match err {
        Error::NotAligned => "not aligned",
        Error::OutOfBounds => "out of bounds",
        Error::Timeout => at,
        Error::Rejected => at,
        Error::Unsupported => "unsupported die",
    }
}

/// Stay halted without the watchdog resetting the part; a reset would clear
/// the RTT block and the log with it.
fn halt() -> ! {
    loop {
        hal::pac::SYS.iwdg_kr().write_value(hal::pac::sys::regs::IwdgKr(0xAAAA));
    }
}
