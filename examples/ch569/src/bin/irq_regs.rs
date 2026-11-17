//! Are GPRs preserved across a SysTick interrupt on CH569 (QingKe V3A)?
//!
//! qingke-rt relies on the hardware stack push and its trampoline only saves
//! `ra`. The embassy time driver's tasks lost `a0` to the blank-flash sentinel,
//! so this checks the claim directly: Phase A spins with interrupts masked,
//! Phase B with a 1 kHz SysTick running.
//!
//! ```console
//! cargo build --release --no-default-features --bin irq_regs
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 8
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use panic_halt as _;
use qingke::pfic;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const PERIOD: u32 = 120_000; // 1 ms at 120 MHz

const WANT_A0: u32 = 0x1234_5678;
const WANT_A2: u32 = 0x0bad_f00d;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn rearm() {
    pac::SYSTICK.cmplr().write(|w| w.set_cmpl(PERIOD));
    pac::SYSTICK.cmphr().write(|w| w.set_cmph(0));
    pac::SYSTICK.ctlr().modify(|w| w.set_streload(true));
}

#[qingke_rt::interrupt(core)]
fn SysTick() {
    pac::SYSTICK.cntfg().modify(|w| w.set_cntif(false));
    rearm();
    unsafe {
        let hits = core::ptr::read_volatile(SCRATCH.add(3));
        core::ptr::write_volatile(SCRATCH.add(3), hits.wrapping_add(1));
    }
}

/// Spin for about a millisecond with known `a0`/`a2`, then report them.
fn spin_check(slot: usize) -> (u32, u32) {
    let v0: u32;
    let v2: u32;
    unsafe {
        core::arch::asm!(
            "li a0, {a0}",
            "li a2, {a2}",
            "li t3, 60000",
            "1:",
            "addi t3, t3, -1",
            "bnez t3, 1b",
            a0 = const WANT_A0,
            a2 = const WANT_A2,
            out("a0") v0,
            out("a2") v2,
            out("t3") _,
        );
    }
    scratch(slot, v0);
    scratch(slot + 1, v2);
    (v0, v2)
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x4952_5152); // "IRQR"
    for i in 1..8 {
        scratch(i, 0);
    }

    // Phase A: no interrupts can be taken while the known values are live.
    unsafe { core::arch::asm!("csrci mstatus, 0x8") };
    let (a, b) = spin_check(4);
    scratch(1, (a != WANT_A0) as u32 | (((b != WANT_A2) as u32) << 1));

    // Phase B: 1 kHz SysTick at priority 0x80, then the same check.
    let stk = &pac::SYSTICK;
    stk.ctlr().modify(|w| {
        w.set_ste(false);
        w.set_stie(false);
    });
    stk.cntfg().modify(|w| w.set_cntif(false));
    rearm();
    stk.ctlr().modify(|w| {
        w.set_stclk(true);
        w.set_ste(true);
        w.set_stie(true);
    });
    unsafe {
        pfic::set_priority(qingke_rt::CoreInterrupt::SysTick as u8, 0x80);
        pfic::enable_interrupt(qingke_rt::CoreInterrupt::SysTick as u8);
        core::arch::asm!("csrs mstatus, 0x8");
    }

    for round in 0..64u32 {
        let (a, b) = spin_check(4);
        scratch(2, round);
        if a != WANT_A0 || b != WANT_A2 {
            scratch(6, a);
            scratch(7, b);
            break;
        }
    }

    loop {
        core::hint::spin_loop();
    }
}
