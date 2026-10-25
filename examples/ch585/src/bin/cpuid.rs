//! Print the core's identity: `misa`, `mvendorid` and friends.
//!
//! ```console
//! cargo run --release --bin cpuid
//! ```
//!
//! These are the standard RISC-V machine-mode CSRs, and they answer questions
//! the datasheet does not state directly — which ISA extensions the silicon
//! actually has, and who made it.
//!
//! # What this part reports (measured)
//!
//! ```text
//! mvendorid (0xf11) = 0x00000000   not a JEDEC vendor id
//! marchid   (0xf12) = 0xdc68d863
//! mimpid    (0xf13) = 0xdc688001
//! mhartid   (0xf14) = 0x00000000
//! misa      (0x301) = 0x40901106   RV32, extensions B C I M U X
//! ```
//!
//! The `misa` extensions are worth knowing: there is **no A** (atomics), which
//! is why the HAL builds for `riscv32imc` and routes every read-modify-write
//! through `portable-atomic`. `B` (bit manipulation) and `X` (WCH's custom
//! extension) are present but the compiler is not told about them.
//!
//! `mvendorid` reading zero means "not a registered JEDEC vendor", not "no
//! vendor" — WCH identifies itself through `marchid`/`mimpid` instead.
//!
//! # Probing safely
//!
//! The RISC-V spec lets a core leave any of these unimplemented, and reading an
//! unimplemented CSR raises an illegal-instruction exception. qingke-rt routes
//! those to `ExceptionHandler`, which its linker script `PROVIDE`s as the
//! infinite-loop `DefaultExceptionHandler` — so on a part that lacks a CSR this
//! program would hang rather than report it.
//!
//! [`ExceptionHandler`] below overrides that weak alias: it marks the probe as
//! having faulted and steps `mepc` past the offending `csrr`, so an absent CSR
//! is reported as `present=false`. All five happen to be implemented on the
//! QingKe V3C, but the probe costs little and keeps the output honest on other
//! parts.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::info;
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

// CSR numbers, from the RISC-V privileged spec. `csrr` takes an immediate, so
// these appear as literals at each use rather than as `const`s.
//
//   mvendorid 0xf11   marchid 0xf12   mimpid 0xf13
//   mhartid   0xf14   misa    0x301   mepc   0x341

/// Set by [`ExceptionHandler`] when a probed read trapped.
static mut TRAP_HIT: u32 = 0;

/// Read a CSR. `csrr` takes an immediate, hence the literal argument.
macro_rules! csrr {
    ($addr:literal) => {{
        let v: u32;
        unsafe { core::arch::asm!(concat!("csrr {}, ", $addr), out(reg) v) };
        v
    }};
}

/// Overrides qingke-rt's weak `ExceptionHandler`.
///
/// Records that the read faulted and resumes past the faulting instruction, so
/// an unimplemented CSR shows up as absent instead of hanging the program.
#[unsafe(no_mangle)]
pub extern "C" fn ExceptionHandler() {
    let epc = csrr!("0x341"); // mepc

    // Compressed instructions are 2 bytes wide, everything else 4.
    let halfword = unsafe { core::ptr::read_volatile(epc as *const u16) } as u32;
    let len = if halfword & 0b11 == 0b11 { 4 } else { 2 };

    unsafe {
        core::ptr::write_volatile(&raw mut TRAP_HIT, 1);
        core::arch::asm!("csrw mepc, {}", in(reg) epc + len);
    }
}

/// Read a CSR, reporting whether it exists: `(value, present)`.
macro_rules! probe_csr {
    ($addr:literal) => {{
        unsafe { core::ptr::write_volatile(&raw mut TRAP_HIT, 0) };
        let mut v: u32 = 0;
        unsafe { core::arch::asm!(concat!("csrr {0}, ", $addr), inout(reg) v) };
        let present = unsafe { core::ptr::read_volatile(&raw const TRAP_HIT) } == 0;
        (v, present)
    }};
}

/// The single-letter names of the extensions `misa` can advertise, indexed by
/// bit position: bit 0 is `A`, bit 1 `B`, … bit 25 `Z`.
fn misa_letters(misa: u32) -> ([u8; 26], usize) {
    let mut buf = [0u8; 26];
    let mut n = 0;
    for bit in 0..26u32 {
        if misa & (1 << bit) != 0 {
            buf[n] = b'A' + bit as u8;
            n += 1;
        }
    }
    (buf, n)
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    info!("ch5xx-hal CPU identity");

    let (vendor, vendor_ok) = probe_csr!("0xf11");
    info!("mvendorid (0xf11) = {=u32:#010x} present={}", vendor, vendor_ok);
    if vendor_ok {
        if vendor == 0 {
            // Per the spec, zero means "not a registered JEDEC vendor", which
            // is not the same as "unknown vendor".
            info!("  -> not a JEDEC vendor id (0 means unregistered)");
        } else {
            info!("  -> JEDEC bank={=u32} offset={=u32}", vendor >> 7, vendor & 0x7f);
        }
    }

    let (arch, arch_ok) = probe_csr!("0xf12");
    info!("marchid   (0xf12) = {=u32:#010x} present={}", arch, arch_ok);

    let (imp, imp_ok) = probe_csr!("0xf13");
    info!("mimpid    (0xf13) = {=u32:#010x} present={}", imp, imp_ok);

    let (hart, hart_ok) = probe_csr!("0xf14");
    info!("mhartid   (0xf14) = {=u32} present={}", hart, hart_ok);

    let (misa, misa_ok) = probe_csr!("0x301");
    info!("misa      (0x301) = {=u32:#010x} present={}", misa, misa_ok);

    if misa_ok {
        // MXL (bits 31:30): 1 = RV32, 2 = RV64.
        match (misa >> 30) & 0x3 {
            1 => info!("  XLEN        = 32"),
            2 => info!("  XLEN        = 64"),
            other => info!("  XLEN        = reserved ({})", other),
        }

        let (buf, n) = misa_letters(misa);
        match core::str::from_utf8(&buf[..n]) {
            Ok(s) => info!("  extensions  = {}", s),
            Err(_) => info!("  extensions  = <not ascii>"),
        }

        info!("  I (base)    = {}", misa & (1 << 8) != 0);
        info!("  M (mul/div) = {}", misa & (1 << 12) != 0);
        info!("  C (16-bit)  = {}", misa & (1 << 2) != 0);
        // No A: this is why the HAL targets riscv32imc and uses portable-atomic.
        info!("  A (atomics) = {}", misa & (1 << 0) != 0);
        info!("  U (user)    = {}", misa & (1 << 20) != 0);
        info!("  B (bitmanip)= {}", misa & (1 << 1) != 0);
        info!("  X (custom)  = {}", misa & (1 << 23) != 0);
    }

    loop {
        core::hint::spin_loop();
    }
}
