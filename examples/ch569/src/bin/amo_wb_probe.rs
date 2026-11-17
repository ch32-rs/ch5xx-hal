//! AMO write-back check, including the `rd == rs1` form the executor's
//! `fetch_or` compiles to (`amoor.w.aqrl a1, a2, (a1)`).
//!
//! Every AMO's returned old value and the resulting memory word are compared
//! against a model kept in `s1`. The first disagreement records
//! `(form, iteration, expected, actual)` at 0x20021008..0x20021014 and
//! `0xdead0000 | form` at 0x20021004.
//!
//! ```console
//! cargo build --release --no-default-features --bin amo_wb_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 8
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

use core::ptr::write_volatile;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const CELL: u32 = 0x2002_1040;

core::arch::global_asm!(
    r#"
    .section .text
    .global _amo_loop
    .align 2
_amo_loop:
    li   s0, 0                /* iteration */
    li   s1, 0                /* model */
    li   a0, 0x20021040
    sw   zero, 0(a0)
1:
    /* form 1: amoor, rd != rs1 */
    li   a0, 0x20021040
    li   t0, 1
    amoor.w.aqrl a1, t0, (a0)
    bne  a1, s1, 11f
    or   t2, s1, t0
    lw   t1, 0(a0)
    bne  t1, t2, 12f
    mv   s1, t2

    /* form 2: amoor, rd == rs1 */
    li   a0, 0x20021040
    li   t0, 2
    amoor.w.aqrl a0, t0, (a0)
    bne  a0, s1, 13f
    or   t2, s1, t0
    li   a0, 0x20021040
    lw   t1, 0(a0)
    bne  t1, t2, 14f
    mv   s1, t2

    /* form 3: amoand, rd == rs1 */
    li   t0, 6
    amoand.w.aqrl a0, t0, (a0)
    bne  a0, s1, 15f
    and  t2, s1, t0
    li   a0, 0x20021040
    lw   t1, 0(a0)
    bne  t1, t2, 16f
    mv   s1, t2

    /* form 4: amoswap, rd == rs1 */
    li   t0, 0x1234
    amoswap.w.aqrl a0, t0, (a0)
    bne  a0, s1, 17f
    li   a0, 0x20021040
    lw   t1, 0(a0)
    bne  t1, t0, 18f
    mv   s1, t0

    /* form 5: amoadd, rd == rs1 */
    li   t0, 1
    amoadd.w.aqrl a0, t0, (a0)
    bne  a0, s1, 19f
    li   a0, 0x20021040
    lw   t1, 0(a0)
    addi t2, s1, 1
    bne  t1, t2, 20f
    mv   s1, t2

    /* form 6: lr/sc pair on the same register as the address */
    li   a0, 0x20021040
    lr.w.aq a1, (a0)
    bne  a1, s1, 21f
    li   t0, 0
    sc.w.rl t0, a0, (a0)
    li   a0, 0x20021040
    lw   t1, 0(a0)
    bne  t1, a0, 22f
    mv   s1, a0

    addi s0, s0, 1
    li   a0, 0x20021000
    sw   s0, 4(a0)
    j    1b

    /* failures: t0 = form, t1 = actual, t2 = expected */
11: li t0, 1
    mv t1, a1
    mv t2, s1
    j  90f
12: li t0, 2
    mv t2, s1
    j  91f
13: li t0, 3
    mv t1, a0
    mv t2, s1
    j  90f
14: li t0, 4
    j  91f
15: li t0, 5
    mv t1, a0
    mv t2, s1
    j  90f
16: li t0, 6
    j  91f
17: li t0, 7
    mv t1, a0
    mv t2, s1
    j  90f
18: li t0, 8
    mv t2, t0
    j  91f
19: li t0, 9
    mv t1, a0
    mv t2, s1
    j  90f
20: li t0, 10
    j  91f
21: li t0, 11
    mv t1, a1
    mv t2, s1
    j  90f
22: li t0, 12
    j  91f
90: li a0, 0x20021000
    sw s0, 8(a0)
    sw t2, 12(a0)
    sw t1, 16(a0)
    j  92f
91: li a0, 0x20021000
    sw s0, 8(a0)
    sw s1, 12(a0)
    sw t1, 16(a0)
92: li a0, 0x20021000
    sw t0, 20(a0)
    li t0, 1
    sw t0, 24(a0)
93: j 93b
"#
);

extern "C" {
    fn _amo_loop() -> !;
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    unsafe {
        write_volatile(SCRATCH, u32::from_le_bytes(*b"AMOW"));
        for i in 1..8 {
            write_volatile(SCRATCH.add(i), 0);
        }
    }

    unsafe { _amo_loop() }
}
