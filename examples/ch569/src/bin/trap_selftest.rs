//! Validates the `trap_probe` dump path: performs a deliberate misaligned
//! store and records mcause/mepc/mtval to RAMX.
//!
//! ```console
//! cargo build --release --bin trap_selftest
//! wlink -d ... --chip CH56X flash -e target/.../trap_selftest
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 16
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke::register::mtvec::{self, TrapMode};
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

core::arch::global_asm!(
    r#"
    .section .text
    .global _trap_dump
    .align 2
_trap_dump:
    la   t3, 0x20021000
    csrr t4, mcause
    sw   t4, 0x00(t3)
    csrr t4, mepc
    sw   t4, 0x04(t3)
    csrr t4, mtval
    sw   t4, 0x08(t3)
    sw   a0, 0x0c(t3)
    sw   ra, 0x10(t3)
    sw   sp, 0x14(t3)
1:
    j 1b
"#
);

extern "C" {
    fn _trap_dump();
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    unsafe {
        mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct);
        core::ptr::write_volatile(SCRATCH.add(6), 0x5e1f_7e57);

        // `sw` to an odd address: store-address-misaligned (mcause = 6).
        core::arch::asm!("sw {v}, 0({p})", v = in(reg) 0x1234_5678u32, p = in(reg) 0x2002_1045usize);
    }

    unsafe { core::ptr::write_volatile(SCRATCH.add(7), 0xdead_beef) };
    loop {
        core::hint::spin_loop();
    }
}
