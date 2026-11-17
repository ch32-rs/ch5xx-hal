//! Characterises the core's misaligned and unmapped accesses.
//!
//! A resumable trap handler logs `(mcause, mepc, mtval)` and steps `mepc` past
//! the faulting instruction, so one boot runs the whole matrix: `lw`/`sw`/AMO at
//! an odd address in RAMX, then the same at an unmapped address, then an
//! aligned unmapped read. Progress markers at 0x20021010 show how far it got.
//!
//! ```console
//! cargo build --release --bin align_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021010 8
//! probe-rs read --chip CH569 --probe ... b32 0x20022000 16
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
    .global _trap_resume
    .align 2
_trap_resume:
    la   a6, 0x20021800
    sw   t0, 0x00(a6)
    sw   t1, 0x04(a6)
    sw   t2, 0x08(a6)
    sw   t3, 0x0c(a6)
    sw   t4, 0x10(a6)
    sw   t5, 0x14(a6)

    la   t0, 0x20022000
    lw   t1, 0x00(t0)
    addi t2, t1, 1
    sw   t2, 0x00(t0)
    slli t1, t1, 4
    add  t1, t1, t0
    csrr t2, mcause
    sw   t2, 0x10(t1)
    csrr t2, mepc
    sw   t2, 0x14(t1)
    csrr t2, mtval
    sw   t2, 0x18(t1)

    csrr t2, mepc
    lhu  t3, 0x00(t2)
    andi t3, t3, 3
    li   t4, 3
    bne  t3, t4, 1f
    addi t2, t2, 4
    j    2f
1:  addi t2, t2, 2
2:  csrw mepc, t2

    la   a6, 0x20021800
    lw   t0, 0x00(a6)
    lw   t1, 0x04(a6)
    lw   t2, 0x08(a6)
    lw   t3, 0x0c(a6)
    lw   t4, 0x10(a6)
    lw   t5, 0x14(a6)
    mret

    .global _align_test
    .align 2
_align_test:
    la   t0, 0x20021010

    li   t1, 0x20021045          # 1: lw,  odd, mapped
    lw   t2, 0x00(t1)
    li   t3, 1
    sw   t3, 0x00(t0)

    li   t1, 0x20021046          # 2: sw,  odd, mapped
    sw   t3, 0x00(t1)
    li   t3, 2
    sw   t3, 0x00(t0)

    li   t1, 0x20021045          # 3: AMO, odd, mapped
    li   t2, 1
    amoor.w.aqrl zero, t2, (t1)
    li   t3, 3
    sw   t3, 0x00(t0)

    li   t1, 0xf7f9bf29          # 4: AMO, odd, unmapped
    amoor.w.aqrl zero, t2, (t1)
    li   t3, 4
    sw   t3, 0x00(t0)

    li   t1, 0xf7f9bf2d          # 5: lw,  odd, unmapped
    lw   t2, 0x00(t1)
    li   t3, 5
    sw   t3, 0x00(t0)

    li   t1, 0xf7f9bf35          # 6: lw,  odd, unmapped (crash site)
    lw   t2, 0x00(t1)
    li   t3, 6
    sw   t3, 0x00(t0)

    li   t1, 0xf7f9bf34          # 7: lw,  aligned, unmapped
    lw   t2, 0x00(t1)
    la   t3, 0x20021080
    sw   t2, 0x00(t3)
    li   t3, 7
    sw   t3, 0x00(t0)

    li   t1, 0xf7f9bf34          # 8: sw,  aligned, unmapped
    li   t2, 0x12345678
    sw   t2, 0x00(t1)
    li   t3, 8
    sw   t3, 0x00(t0)
    ret
"#
);

extern "C" {
    fn _trap_resume();
    fn _align_test();
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    unsafe {
        let log = 0x2002_2000 as *mut u32;
        for i in 0..48 {
            core::ptr::write_volatile(log.add(i), 0);
        }
        let marks = 0x2002_1010 as *mut u32;
        for i in 0..8 {
            core::ptr::write_volatile(marks.add(i), 0);
        }
        core::ptr::write_volatile(0x2002_1080 as *mut u32, 0);

        mtvec::write(_trap_resume as *const () as usize, TrapMode::Direct);
        _align_test();
    }

    loop {
        core::hint::spin_loop();
    }
}
