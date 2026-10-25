//! Print and decode the core's identity CSRs: `misa`, `mvendorid`, `marchid`,
//! `mimpid` and `mhartid`.
//!
//! ```console
//! cargo run --release --bin cpuid
//! ```
//!
//! The decoding rules come from WCH's *QingKeV3 Processor Manual*: section 8.2
//! covers the standard RISC-V CSRs, and chapter 9 the V3 series variants and
//! sub-versions. The manual is available only as a PDF in `~/WCH`; AGENTS.md
//! records how to extract it.
//!
//! # What this part reports (measured)
//!
//! ```text
//! mvendorid (0xf11) = 0x00000000   V3C does not implement it (V3F only)
//! marchid   (0xf12) = 0xdc68d863   T31 T15 T14:1 | T10 T9 T5 | T4 T0
//!                                  = W C H | V | 3 | C   -> "WCH V3C"
//! mimpid    (0xf13) = 0xdc688001   WCH | reserved | minor 1
//! mhartid   (0xf14) = 0x00000000   V3C does not implement it (V3F only)
//! misa      (0x301) = 0x40901106   RV32, extensions B C I M U X
//! ```
//!
//! # Decoding `marchid` and `mimpid`
//!
//! Both pack a vendor string into three 5-bit letter codes, using the manual's
//! table 8-1 (`A`=1 … `Z`=26):
//!
//! | `marchid` | | | `mimpid` | |
//! | --- | --- | --- | --- | --- |
//! | `[30:26]` | `Vender0` | `0x17` = W | `[30:26]` | `Vender0` |
//! | `[25:21]` | `Vender1` | `0x03` = C | `[25:21]` | `Vender1` |
//! | `[20:16]` | `Vender2` | `0x08` = H | `[20:16]` | `Vender2` |
//! | `[14:10]` | `Arch` | `0x16` = V | `[14:8]` | reserved |
//! | `[9:5]` | `Serial` | `0x03` = 3 | `[7:0]` | `Minor` |
//! | `[4:0]` | `Version` | `0x03` = C | | |
//!
//! `marchid` therefore spells **WCH V3C** — vendor, architecture, series and
//! silicon revision. Bit 31 and bit 15 read 1 and are documented reserved.
//!
//! `mimpid[7:0]` is the sub-version. Chapter 9 lists what each value adds:
//! 2 = software reset, software interrupt and nesting control; 3 = the custom
//! delay instruction and `zicond`. Its entry for 1 is not a feature list but
//! two part numbers — "CH32M030, CH585" — with nothing that tells them apart.
//! So reading 1 here is *consistent with* a CH585 and does not identify it;
//! `marchid` is the CSR that names the core.
//!
//! `mvendorid` and `mhartid` are documented as valid on **V3F only**; V3C reads
//! zero. That is the core saying "not applicable", not an absent CSR — the
//! `present` flag below stays true. `misa` and `marchid`/`mimpid` work on V3C.
//!
//! # `misa` is load-bearing
//!
//! There is **no A** (atomics), which is why the HAL builds for `riscv32imc` and
//! routes every read-modify-write through `portable-atomic`. `X` is reported and
//! appears to be the WCH compressed loads/stores that `+xwchc` asks for.
//!
//! `B` is reported too, but `misa` cannot say *which* B: table 8-2 calls bit 1
//! "Tentatively reserved for Bit-Manipulation extension", and the ratified
//! `Zb*` split, the pre-ratification draft and a WCH subset all encode
//! differently. The `bitmanip` binary settles it on hardware: every
//! ratified `Zba`/`Zbb`/`Zbs` instruction executes, plus `Zbc`'s `clmul`. That
//! is why the examples pass `+b` in `rustflags`.
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
//! is reported as `present=false`. All five are readable here (two just return
//! zero), but the probe keeps the output honest on other parts.

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

/// Turn the three 5-bit vendor fields into the letters they encode.
///
/// The manual's table 8-1 maps `A`..`Z` to 1..26. A zero field is skipped
/// rather than shown as a stray character, so a partially-used field reads
/// sensibly.
fn vendor_letters(v0: u32, v1: u32, v2: u32) -> ([u8; 3], usize) {
    let mut buf = [0u8; 3];
    let mut n = 0;
    for v in [v0, v1, v2] {
        if (1..=26).contains(&v) {
            buf[n] = b'A' + (v - 1) as u8;
            n += 1;
        }
    }
    (buf, n)
}

/// View a byte buffer as text, or `"?"` if it is not ASCII.
fn ascii(b: &[u8]) -> &str {
    core::str::from_utf8(b).unwrap_or("?")
}

/// Letter for a single 5-bit code, per table 8-1.
fn letter(code: u32) -> u8 {
    if (1..=26).contains(&code) {
        b'A' + (code - 1) as u8
    } else {
        b'?'
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    info!("ch5xx-hal CPU identity");

    let (vendor, vendor_ok) = probe_csr!("0xf11");
    info!("mvendorid (0xf11) = {=u32:#010x} present={}", vendor, vendor_ok);
    if vendor_ok && vendor == 0 {
        // The manual documents this CSR as V3F-only; V3C reads zero.
        info!("  -> V3C does not implement it (V3F only), not an absent CSR");
    }

    // --- marchid: vendor | arch | series | revision -----------------------
    let (arch, arch_ok) = probe_csr!("0xf12");
    info!("marchid   (0xf12) = {=u32:#010x} present={}", arch, arch_ok);
    if arch_ok && arch != 0 {
        let (v0, v1, v2) = ((arch >> 26) & 0x1f, (arch >> 21) & 0x1f, (arch >> 16) & 0x1f);
        let a = (arch >> 10) & 0x1f; // architecture letter
        let s = (arch >> 5) & 0x1f; // series digit
        let r = arch & 0x1f; // silicon revision letter

        let (vbuf, vn) = vendor_letters(v0, v1, v2);
        info!("  vendor      = {}", ascii(&vbuf[..vn]));
        // Series is a *number*, so print the raw 5-bit value rather than a
        // letter. Version and Arch are letters.
        info!("  arch/series/rev = {}{}{}", letter(a) as char, s, letter(r) as char);
        info!(
            "  raw fields  = V0={=u32:#x} V1={=u32:#x} V2={=u32:#x} Arch={=u32:#x} Serial={=u32:#x} Ver={=u32:#x}",
            v0, v1, v2, a, s, r
        );
    }

    // --- mimpid: vendor | reserved | minor --------------------------------
    let (imp, imp_ok) = probe_csr!("0xf13");
    info!("mimpid    (0xf13) = {=u32:#010x} present={}", imp, imp_ok);
    if imp_ok && imp != 0 {
        let (v0, v1, v2) = ((imp >> 26) & 0x1f, (imp >> 21) & 0x1f, (imp >> 16) & 0x1f);
        let minor = imp & 0xff;
        let (vbuf, vn) = vendor_letters(v0, v1, v2);
        info!("  vendor      = {}", ascii(&vbuf[..vn]));
        info!("  minor       = {}", minor);
        // Chapter 9 lists the sub-versions by the features they add. It does
        // not make this value an identifier: the entry for 1 names two parts
        // (CH32M030 and CH585) without saying which of them is running, so
        // reading 1 here is consistent with a CH585 but does not prove it.
        let what = match minor {
            1 => "entry lists CH32M030 and CH585; no distinguishing feature",
            2 => "software reset, software interrupt, nesting control",
            3 => "custom delay instruction, zicond",
            _ => "not in the chapter 9 list",
        };
        info!("  -> {}", what);
    }

    let (hart, hart_ok) = probe_csr!("0xf14");
    info!("mhartid   (0xf14) = {=u32} present={}", hart, hart_ok);
    if hart_ok && hart == 0 {
        info!("  -> V3C does not implement it (V3F only); single core anyway");
    }

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
        // Reported, but the bit alone does not say whether this is the ratified
        // `Zb*` encoding or the older draft; `bitmanip` answers that on hardware.
        info!("  B (bitmanip)= {}", misa & (1 << 1) != 0);
        info!("  X (custom)  = {}", misa & (1 << 23) != 0);
    }

    loop {
        core::hint::spin_loop();
    }
}
