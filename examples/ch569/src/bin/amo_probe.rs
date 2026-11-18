//! Do RISC-V AMO instructions (`amoor.w`) work on the CH569's QingKe V3A core?

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use panic_halt as _;
use qingke::pfic;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const PERIOD: u32 = 120_000;

// Neighbours of `TARGET` catch a store that lands at the wrong offset.
static CANARY_A: AtomicU32 = AtomicU32::new(0xaaaa_aaaa);
static TARGET: AtomicU32 = AtomicU32::new(0x0f);
static CANARY_B: AtomicU32 = AtomicU32::new(0x5555_5555);

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

#[qingke_rt::interrupt(core)]
fn SysTick() {
    pac::SYSTICK.cmplr().write(|w| w.set_cmpl(PERIOD));
    pac::SYSTICK.cmphr().write(|w| w.set_cmph(0));
    pac::SYSTICK.ctlr().modify(|w| w.set_streload(true));
    pac::SYSTICK.cntfg().modify(|w| w.set_cntif(false));
}

/// Returns a bitmask of failed expectations, one bit per operation.
fn run_ops() -> u32 {
    let mut fail = 0u32;
    TARGET.store(0x0f, Ordering::SeqCst);
    if TARGET.fetch_or(0xf0, Ordering::SeqCst) != 0x0f || TARGET.load(Ordering::SeqCst) != 0xff {
        fail |= 1 << 0;
    }
    if TARGET.fetch_and(0x3c, Ordering::SeqCst) != 0xff || TARGET.load(Ordering::SeqCst) != 0x3c {
        fail |= 1 << 1;
    }
    if TARGET.fetch_xor(0xff, Ordering::SeqCst) != 0x3c || TARGET.load(Ordering::SeqCst) != 0xc3 {
        fail |= 1 << 2;
    }
    if TARGET.swap(0x1234, Ordering::SeqCst) != 0xc3 || TARGET.load(Ordering::SeqCst) != 0x1234 {
        fail |= 1 << 3;
    }
    if TARGET.fetch_add(1, Ordering::SeqCst) != 0x1234 || TARGET.load(Ordering::SeqCst) != 0x1235 {
        fail |= 1 << 4;
    }
    if TARGET.fetch_max(0x8000_0000, Ordering::SeqCst) != 0x1235
        || TARGET.load(Ordering::SeqCst) != 0x8000_0000
    {
        fail |= 1 << 5;
    }
    if CANARY_A.load(Ordering::SeqCst) != 0xaaaa_aaaa
        || CANARY_B.load(Ordering::SeqCst) != 0x5555_5555
    {
        fail |= 1 << 6;
    }
    fail
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x414d_4f50); // "AMOP"
    for i in 1..8 {
        scratch(i, 0);
    }

    // Phase A: interrupts masked.
    unsafe { core::arch::asm!("csrci mstatus, 0x8") };
    scratch(1, run_ops());
    scratch(4, TARGET.load(Ordering::SeqCst));

    // Phase B: repeat with a 1 kHz SysTick running.
    let stk = &pac::SYSTICK;
    stk.ctlr().modify(|w| {
        w.set_ste(false);
        w.set_stie(false);
    });
    stk.cntfg().modify(|w| w.set_cntif(false));
    pac::SYSTICK.cmplr().write(|w| w.set_cmpl(PERIOD));
    pac::SYSTICK.cmphr().write(|w| w.set_cmph(0));
    stk.ctlr().modify(|w| {
        w.set_stclk(true);
        w.set_streload(true);
        w.set_ste(true);
        w.set_stie(true);
    });
    unsafe {
        pfic::set_priority(qingke_rt::CoreInterrupt::SysTick as u8, 0x80);
        pfic::enable_interrupt(qingke_rt::CoreInterrupt::SysTick as u8);
        core::arch::asm!("csrs mstatus, 0x8");
    }

    let mut n = 0u32;
    loop {
        let fail = run_ops();
        n += 1;
        scratch(2, n);
        scratch(3, fail);
        if fail != 0 {
            scratch(5, TARGET.load(Ordering::SeqCst));
            break;
        }
        if n == 100_000 {
            scratch(6, 0x0d0e_0000); // "done"
            break;
        }
    }

    loop {
        core::hint::spin_loop();
    }
}
