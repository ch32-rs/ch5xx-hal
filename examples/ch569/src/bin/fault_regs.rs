//! Is the core's saved context trustworthy for a misaligned-AMO fault?
//!
//! Loads every general register with a magic value, then faults inside
//! `amoor.w` on an odd address — the same form the executor crash produces.
//! The handler dumps a0-a7, s0-s11, sp, ra, gp and tp, so any register the
//! core mangles (or reports stale) shows up as a missing magic.
//!
//! ```console
//! cargo build --release --bin fault_regs
//! probe-rs read --chip CH569 --probe ... b32 0x20022000 27
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke::register::mtvec::{self, TrapMode};
use qingke_rt::entry;

core::arch::global_asm!(
    r#"
    .section .text
    .global _trap_dump
    .align 2
_trap_dump:
    la   t1, 0x20022000
    sw   a0, 0x00(t1)
    sw   a1, 0x04(t1)
    sw   a2, 0x08(t1)
    sw   a3, 0x0c(t1)
    sw   a4, 0x10(t1)
    sw   a5, 0x14(t1)
    sw   a6, 0x18(t1)
    sw   a7, 0x1c(t1)
    sw   s0, 0x20(t1)
    sw   s1, 0x24(t1)
    sw   s2, 0x28(t1)
    sw   s3, 0x2c(t1)
    sw   s4, 0x30(t1)
    sw   s5, 0x34(t1)
    sw   s6, 0x38(t1)
    sw   s7, 0x3c(t1)
    sw   s8, 0x40(t1)
    sw   s9, 0x44(t1)
    sw   s10, 0x48(t1)
    sw   s11, 0x4c(t1)
    csrr t2, mcause
    sw   t2, 0x50(t1)
    csrr t2, mepc
    sw   t2, 0x54(t1)
    csrr t2, mtval
    sw   t2, 0x58(t1)
    sw   sp, 0x5c(t1)
    sw   ra, 0x60(t1)
    sw   gp, 0x64(t1)
    sw   tp, 0x68(t1)
1:  j 1b

    .global _fault_it
    .align 2
_fault_it:
    li   a0, 0xa0
    li   a1, 0xa1
    li   a2, 0xa2
    li   a3, 0xa3
    li   a4, 0xa4
    li   a5, 0xa5
    li   a6, 0xa6
    li   s0, 0x50
    li   s1, 0x51
    li   s2, 0x52
    li   s3, 0x53
    li   s4, 0x54
    li   s5, 0x55
    li   s6, 0x56
    li   s7, 0x57
    li   s8, 0x58
    li   s9, 0x59
    li   s10, 0x5a
    li   s11, 0x5b
    li   tp, 0x7d
    li   ra, 0x5c
    li   gp, 0x6c
    li   a7, 0x20021045
    amoor.w.aqrl zero, zero, (a7)
    ret
"#
);

extern "C" {
    fn _trap_dump();
    fn _fault_it();
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    unsafe {
        let dump = 0x2002_2000 as *mut u32;
        for i in 0..32 {
            core::ptr::write_volatile(dump.add(i), 0);
        }
        mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct);
        _fault_it();
    }

    loop {
        core::hint::spin_loop();
    }
}
