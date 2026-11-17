//! Register-file stress: every callee- and caller-saved register is reloaded
//! with a magic, then compared, in a loop that also writes RAMX each round.
//!
//! a0 and t0 are the only scratch registers. Progress is counted at
//! 0x20021004; the first mismatch stores the register index at 0x20021008, the
//! observed value at 0x2002100c and a stopped flag at 0x20021014.
//!
//! ```console
//! cargo build --release --no-default-features --bin reg_stress_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 6
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

core::arch::global_asm!(
    r#"
    .section .text
    .global _reg_loop
    .align 2
_reg_loop:
    li   a0, 0x20021000
    li   t0, 0
    sw   t0, 4(a0)
1:
    /* progress */
    li   a0, 0x20021000
    lw   t0, 4(a0)
    addi t0, t0, 1
    sw   t0, 4(a0)

    /* magics: s0-s11 = 0x100..0x10b, a1-a7 = 0x201..0x207, t1-t6 = 0x301..0x306 */
    li   s0, 0x100
    li   s1, 0x101
    li   s2, 0x102
    li   s3, 0x103
    li   s4, 0x104
    li   s5, 0x105
    li   s6, 0x106
    li   s7, 0x107
    li   s8, 0x108
    li   s9, 0x109
    li   s10, 0x10a
    li   s11, 0x10b
    li   a1, 0x201
    li   a2, 0x202
    li   a3, 0x203
    li   a4, 0x204
    li   a5, 0x205
    li   a6, 0x206
    li   a7, 0x207
    li   t1, 0x301
    li   t2, 0x302
    li   t3, 0x303
    li   t4, 0x304
    li   t5, 0x305
    li   t6, 0x306
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop

    li   t0, 0x100
    beq  s0, t0, 11f
    li   t0, 0
    j    90f
11: li   t0, 0x101
    beq  s1, t0, 12f
    li   t0, 1
    j    90f
12: li   t0, 0x102
    beq  s2, t0, 13f
    li   t0, 2
    j    90f
13: li   t0, 0x103
    beq  s3, t0, 14f
    li   t0, 3
    j    90f
14: li   t0, 0x104
    beq  s4, t0, 15f
    li   t0, 4
    j    90f
15: li   t0, 0x105
    beq  s5, t0, 16f
    li   t0, 5
    j    90f
16: li   t0, 0x106
    beq  s6, t0, 17f
    li   t0, 6
    j    90f
17: li   t0, 0x107
    beq  s7, t0, 18f
    li   t0, 7
    j    90f
18: li   t0, 0x108
    beq  s8, t0, 19f
    li   t0, 8
    j    90f
19: li   t0, 0x109
    beq  s9, t0, 20f
    li   t0, 9
    j    90f
20: li   t0, 0x10a
    beq  s10, t0, 21f
    li   t0, 10
    j    90f
21: li   t0, 0x10b
    beq  s11, t0, 22f
    li   t0, 11
    j    90f
22: li   t0, 0x201
    beq  a1, t0, 23f
    li   t0, 12
    j    90f
23: li   t0, 0x202
    beq  a2, t0, 24f
    li   t0, 13
    j    90f
24: li   t0, 0x203
    beq  a3, t0, 25f
    li   t0, 14
    j    90f
25: li   t0, 0x204
    beq  a4, t0, 26f
    li   t0, 15
    j    90f
26: li   t0, 0x205
    beq  a5, t0, 27f
    li   t0, 16
    j    90f
27: li   t0, 0x206
    beq  a6, t0, 28f
    li   t0, 17
    j    90f
28: li   t0, 0x207
    beq  a7, t0, 29f
    li   t0, 18
    j    90f
29: li   t0, 0x301
    beq  t1, t0, 30f
    li   t0, 19
    j    90f
30: li   t0, 0x302
    beq  t2, t0, 31f
    li   t0, 20
    j    90f
31: li   t0, 0x303
    beq  t3, t0, 32f
    li   t0, 21
    j    90f
32: li   t0, 0x304
    beq  t4, t0, 33f
    li   t0, 22
    j    90f
33: li   t0, 0x305
    beq  t5, t0, 34f
    li   t0, 23
    j    90f
34: li   t0, 0x306
    beq  t6, t0, 1b
    li   t0, 24
90: li   a0, 0x20021000
    sw   t0, 8(a0)
    li   t0, 1
    sw   t0, 20(a0)
91: j    91b
"#
);

extern "C" {
    fn _reg_loop() -> !;
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    unsafe {
        core::ptr::write_volatile(SCRATCH, u32::from_le_bytes(*b"RGST"));
        for i in 1..6 {
            core::ptr::write_volatile(SCRATCH.add(i), 0);
        }
    }

    unsafe { _reg_loop() }
}
