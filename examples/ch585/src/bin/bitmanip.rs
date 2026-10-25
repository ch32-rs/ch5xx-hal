//! Which bit-manipulation instructions this core really executes.
//!
//! ```console
//! cargo run --release --bin bitmanip
//! ```
//!
//! # Why this exists
//!
//! `misa` reports bit 1, and the *QingKeV3 Processor Manual* lists the V3C as
//! **RV32IMCB** (§1.1, table 1-1). Neither says *which* B that is. There are
//! three candidates and they are not interchangeable:
//!
//! - the ratified `Zba`/`Zbb`/`Zbs` split (what `+b` means to LLVM today),
//! - the older, pre-ratification `B` draft, whose opcodes and mnemonics differ,
//! - a WCH-selected subset.
//!
//! Table 8-2 hedges — it calls bit 1 "Tentatively reserved for Bit-Manipulation
//! extension" — so documentation alone cannot settle it. The only trustworthy
//! answer is to execute the instructions and see whether they trap.
//!
//! # Result on the CH585
//!
//! All 25 probed instructions execute correctly, including `Zbc`'s `clmul`/
//! `clmulh`, which the ratified `B` collection does **not** include. The core
//! therefore implements the ratified Zb* encodings, and more than `+b` implies.
//!
//! # How it probes
//!
//! The target is plain `riscv32imc`, so the assembler rejects these mnemonics by
//! default. Each probe wraps its instruction in `.option arch, +<zbext>`; that
//! changes only what the assembler accepts, not what the compiler may emit
//! elsewhere in the crate.
//!
//! An illegal instruction raises `mcause = 2` and vectors to
//! [`ExceptionHandler`]. qingke-rt `PROVIDE`s that symbol as an infinite loop,
//! so without an override a missing instruction would hang silently. This
//! program overrides it, records `mcause`, and steps `mepc` past the faulting
//! instruction so the next probe can run.
//!
//! The three **control** lines at the end are as important as the rest. `A` is
//! absent from this part (`misa` bit 0 is clear), so `amoadd.w`, `amoswap.w`
//! and `unimp` must all report `TRAP`. If they do not, the handler is not
//! running and every `OK` above is a false positive.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::info;
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

/// Set by [`ExceptionHandler`] for every faulting probe instruction.
static mut TRAP_HIT: u32 = 0;
/// `mcause` of the last fault, for the report line.
static mut MCAUSE: u32 = 0;

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
/// Records the trap and resumes past the faulting instruction so a probe of a
/// missing instruction reports it instead of hanging the program.
#[unsafe(no_mangle)]
pub extern "C" fn ExceptionHandler() {
    let epc = csrr!("0x341"); // mepc
    let cause = csrr!("0x342"); // mcause

    // Compressed instructions are 2 bytes wide, everything else 4.
    let halfword = unsafe { core::ptr::read_volatile(epc as *const u16) } as u32;
    let len = if halfword & 0b11 == 0b11 { 4 } else { 2 };

    unsafe {
        core::ptr::write_volatile(&raw mut TRAP_HIT, 1);
        core::ptr::write_volatile(&raw mut MCAUSE, cause);
        core::arch::asm!("csrw mepc, {}", in(reg) epc + len);
    }
}

/// Run `$body` and report whether it trapped.
///
/// `$ext` is the sub-extension that defines the instruction. It is needed only
/// so the assembler accepts the mnemonic; `riscv32imc` does not enable any Zb*.
macro_rules! try_binop {
    ($name:literal, $ext:literal, $body:literal) => {{
        let mut a: u32 = 0xF0F0_0F0F;
        let mut b: u32 = 4;
        unsafe {
            core::ptr::write_volatile(&raw mut TRAP_HIT, 0);
            core::arch::asm!(
                ".option push",
                concat!(".option arch, +", $ext),
                $body,
                ".option pop",
                inout(reg) a,
                in(reg) b,
            );
        }
        report($name, unsafe { core::ptr::read_volatile(&raw const TRAP_HIT) } == 0, Some(a));
    }};
}

/// Same, for instructions that take one register.
macro_rules! try_unop {
    ($name:literal, $ext:literal, $body:literal) => {{
        let mut a: u32 = 0xF0F0_0F0F;
        unsafe {
            core::ptr::write_volatile(&raw mut TRAP_HIT, 0);
            core::arch::asm!(
                ".option push",
                concat!(".option arch, +", $ext),
                $body,
                ".option pop",
                inout(reg) a,
            );
        }
        report($name, unsafe { core::ptr::read_volatile(&raw const TRAP_HIT) } == 0, Some(a));
    }};
}

/// Control probe for an instruction this part is known to lack.
macro_rules! try_control {
    ($name:literal, $ext:literal, $body:literal) => {{
        static mut CELL: u32 = 0;
        unsafe {
            core::ptr::write_volatile(&raw mut TRAP_HIT, 0);
            let p = &raw mut CELL;
            core::arch::asm!(
                ".option push",
                concat!(".option arch, +", $ext),
                $body,
                ".option pop",
                in(reg) p,
            );
        }
        report($name, unsafe { core::ptr::read_volatile(&raw const TRAP_HIT) } == 0, None);
    }};
}

/// Control probe for a guaranteed-illegal instruction.
macro_rules! try_unimp {
    ($name:literal) => {{
        unsafe {
            core::ptr::write_volatile(&raw mut TRAP_HIT, 0);
            core::arch::asm!("unimp");
        }
        report(
            $name,
            unsafe { core::ptr::read_volatile(&raw const TRAP_HIT) } == 0,
            None,
        );
    }};
}

/// Print one probe's verdict. A control that *succeeds* is the alarming case.
fn report(name: &str, ok: bool, value: Option<u32>) {
    match (ok, value) {
        (true, Some(a)) => info!("{=str} OK    a={=u32:#010x}", name, a),
        (true, None) => info!("{=str} OK    <-- DETECTION BROKEN", name),
        (false, _) => {
            let c = unsafe { core::ptr::read_volatile(&raw const MCAUSE) };
            info!("{=str} TRAP  mcause={=u32:#x}", name, c);
        }
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());
    info!("bit-manipulation probe (misa bit 1 = {})", csrr!("0x301") & 2 != 0);

    // Zbb: basic bit manipulation.
    try_binop!("andn", "zbb", "andn {0}, {0}, {1}");
    try_binop!("orn", "zbb", "orn {0}, {0}, {1}");
    try_binop!("xnor", "zbb", "xnor {0}, {0}, {1}");
    try_unop!("clz", "zbb", "clz {0}, {0}");
    try_unop!("ctz", "zbb", "ctz {0}, {0}");
    try_unop!("cpop", "zbb", "cpop {0}, {0}");
    try_binop!("max", "zbb", "max {0}, {0}, {1}");
    try_binop!("minu", "zbb", "minu {0}, {0}, {1}");
    try_unop!("sext.b", "zbb", "sext.b {0}, {0}");
    try_unop!("sext.h", "zbb", "sext.h {0}, {0}");
    try_unop!("zext.h", "zbb", "zext.h {0}, {0}");
    try_binop!("rol", "zbb", "rol {0}, {0}, {1}");
    try_binop!("ror", "zbb", "ror {0}, {0}, {1}");
    try_unop!("orc.b", "zbb", "orc.b {0}, {0}");
    try_unop!("rev8", "zbb", "rev8 {0}, {0}");

    // Zba: address generation.
    try_binop!("sh1add", "zba", "sh1add {0}, {0}, {1}");
    try_binop!("sh2add", "zba", "sh2add {0}, {0}, {1}");
    try_binop!("sh3add", "zba", "sh3add {0}, {0}, {1}");

    // Zbs: single-bit instructions.
    try_binop!("bset", "zbs", "bset {0}, {0}, {1}");
    try_binop!("bclr", "zbs", "bclr {0}, {0}, {1}");
    try_binop!("binv", "zbs", "binv {0}, {0}, {1}");
    try_binop!("bext", "zbs", "bext {0}, {0}, {1}");

    // Zbc: carry-less multiply. Outside the ratified `B` collection, so these
    // are what distinguish "implements +b" from "implements more than +b".
    // `clmulr` is the third Zbc opcode and is what `+zbc` adds on top of `+b`;
    // LLVM may emit it for `u32::carryless_mul` (still an unstable intrinsic),
    // so it has to be probed before `+zbc` can be enabled.
    try_binop!("clmul", "zbc", "clmul {0}, {0}, {1}");
    try_binop!("clmulh", "zbc", "clmulh {0}, {0}, {1}");
    try_binop!("clmulr", "zbc", "clmulr {0}, {0}, {1}");

    // Controls. There is no A extension, so all three must trap. If they run,
    // the handler is not installed and every OK above is meaningless.
    try_control!("amoadd.w (control)", "a", "amoadd.w zero, zero, ({0})");
    try_control!("amoswap.w (control)", "a", "amoswap.w zero, zero, ({0})");
    try_unimp!("unimp (control)");

    info!("done");
    loop {
        core::hint::spin_loop();
    }
}
