//! Does an interrupt preserve the caller-saved registers?

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use hal::interrupt::typelevel::{Handler, Interrupt};
use hal::timer::counter::Counter;
use hal::timer::{Instance, PeriodTicks};
use panic_halt as _;
use qingke_rt::entry;

hal::bind_interrupts!(struct Irqs0 { TMR0 => TmrIrq; });

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

struct TmrIrq;

impl Handler<hal::interrupt::typelevel::TMR0> for TmrIrq {
    unsafe fn on_interrupt() {
        let r = unsafe { hal::pac::tmr::TmrFull::from_ptr(hal::pac::TMR0.as_ptr()) };
        r.int_flag().write(|w| w.0 = 1);
        let count = SCRATCH.add(8) as *mut u32;
        unsafe { count.write_volatile(count.read_volatile().wrapping_add(1)) };
    }
}

core::arch::global_asm!(
    r#"
    .section .text
    .global _reg_loop
    .align 2
_reg_loop:
    li   s0, 300000
    li   s3, 0
    li   t0, 0x20021000
    li   t1, 0xabcd
    sw   t1, 24(t0)
1:
    addi s3, s3, 1
    li   t0, 0x20021000
    sw   s3, 4(t0)
    li   a1, 0x101
    li   a2, 0x102
    li   a3, 0x103
    li   a4, 0x104
    li   a5, 0x105
    li   a6, 0x106
    li   a7, 0x107
    li   t1, 0x201
    li   t2, 0x202
    li   t3, 0x203
    li   t4, 0x204
    li   t5, 0x205
    li   t6, 0x206
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    nop
    li   t0, 0x101
    beq  a1, t0, 11f
    li   s1, 1
    mv   s2, a1
    j    90f
11: li   t0, 0x102
    beq  a2, t0, 12f
    li   s1, 2
    mv   s2, a2
    j    90f
12: li   t0, 0x103
    beq  a3, t0, 13f
    li   s1, 3
    mv   s2, a3
    j    90f
13: li   t0, 0x104
    beq  a4, t0, 14f
    li   s1, 4
    mv   s2, a4
    j    90f
14: li   t0, 0x105
    beq  a5, t0, 15f
    li   s1, 5
    mv   s2, a5
    j    90f
15: li   t0, 0x106
    beq  a6, t0, 16f
    li   s1, 6
    mv   s2, a6
    j    90f
16: li   t0, 0x107
    beq  a7, t0, 17f
    li   s1, 7
    mv   s2, a7
    j    90f
17: li   t0, 0x201
    beq  t1, t0, 18f
    li   s1, 9
    mv   s2, t1
    j    90f
18: li   t0, 0x202
    beq  t2, t0, 19f
    li   s1, 10
    mv   s2, t2
    j    90f
19: li   t0, 0x203
    beq  t3, t0, 20f
    li   s1, 11
    mv   s2, t3
    j    90f
20: li   t0, 0x204
    beq  t4, t0, 21f
    li   s1, 12
    mv   s2, t4
    j    90f
21: li   t0, 0x205
    beq  t5, t0, 22f
    li   s1, 13
    mv   s2, t5
    j    90f
22: li   t0, 0x206
    beq  t6, t0, 23f
    li   s1, 14
    mv   s2, t6
    j    90f
23: addi s0, s0, -1
    bnez s0, 1b
    li   t0, 0x20021000
    li   t1, 1
    sw   t1, 16(t0)
    sw   s3, 28(t0)
    j    91f
90: li   t0, 0x20021000
    sw   s1, 8(t0)
    sw   s2, 12(t0)
    li   t1, 1
    sw   t1, 20(t0)
91: j    91b
"#
);

extern "C" {
    fn _reg_loop() -> !;
}

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    scratch(0, u32::from_le_bytes(*b"IRQR"));
    for i in 1..8 {
        scratch(i, 0);
    }
    scratch(9, 0);

    let hz = hal::rcc::clocks().sysclk.0;
    let _counter = Counter::new(p.TMR0, PeriodTicks::from_hz(hz, 10_000).unwrap());
    unsafe {
        <hal::peripherals::TMR0 as Instance>::Interrupt::enable();
        let r = hal::pac::tmr::TmrFull::from_ptr(hal::pac::TMR0.as_ptr());
        r.inter_en().write(|w| w.0 = 1);
    }

    scratch(10, unsafe { core::ptr::read_volatile(0xE000_E048 as *const u32) });

    unsafe { _reg_loop() }
}
