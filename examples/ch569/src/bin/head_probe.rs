//! Which access in the executor's dequeue faults when the popped node is bad?
//!
//! Replays the dequeue body with `a0 = 0xf7f9bf11` (the value the run-queue head
//! returned in the crash) and logs every trap through a resumable handler.
//!
//! ```console
//! cargo build --release --bin head_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021010 4
//! probe-rs read --chip CH569 --probe ... b32 0x20022000 32
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

    .global _head_test
    .align 2
_head_test:
    la   t0, 0x20021010

    # Step 1: the dequeue's own loads and AMO with the bad node in a0.
    li   a0, 0xf7f9bf11
    lw   s1, 0x1c(a0)
    li   t3, 1
    sw   t3, 0x00(t0)

    sw   zero, 0x1c(a0)
    li   t3, 2
    sw   t3, 0x00(t0)

    addi a1, a0, 0x18
    li   s5, -0x3
    amoand.w.aqrl zero, s5, (a1)
    li   t3, 3
    sw   t3, 0x00(t0)

    lw   a1, 0x24(a0)
    li   t3, 4
    sw   t3, 0x00(t0)

    # Step 2: same loads at the aligned address 0xf7f9bf10 (control).
    li   a0, 0xf7f9bf10
    lw   s1, 0x1c(a0)
    li   t3, 5
    sw   t3, 0x00(t0)

    lw   a1, 0x24(a0)
    li   t3, 6
    sw   t3, 0x00(t0)
    ret
"#
);

extern "C" {
    fn _trap_resume();
    fn _head_test();
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
        for i in 0..4 {
            core::ptr::write_volatile(marks.add(i), 0);
        }
        mtvec::write(_trap_resume as *const () as usize, TrapMode::Direct);
        _head_test();
    }

    loop {
        core::hint::spin_loop();
    }
}
