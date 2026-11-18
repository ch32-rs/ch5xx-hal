//! What does CH569's SysTick actually do? Vendor setup versus the HAL's.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const PERIOD: u64 = 500_000;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn cnt() -> (u32, u32) {
    let stk = &pac::SYSTICK;
    (stk.cnth().read().cnth(), stk.cntl().read().cntl())
}

fn spin(n: u32) {
    let mut i = 0;
    while i < n {
        i = i.wrapping_add(1);
        core::hint::spin_loop();
    }
}

fn stop_and_clear() {
    let stk = &pac::SYSTICK;
    stk.ctlr().modify(|w| {
        w.set_ste(false);
        w.set_stie(false);
    });
    stk.cntfg().modify(|w| w.set_cntif(false));
    stk.cntl().write(|w| w.set_cntl(0));
    stk.cnth().write(|w| w.set_cnth(0));
    stk.cmplr().write(|w| w.set_cmpl(PERIOD as u32));
    stk.cmphr().write(|w| w.set_cmph((PERIOD >> 32) as u32));
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x5359_5354); // "SYST"
    for i in 1..24 {
        scratch(i, 0);
    }

    let stk = &pac::SYSTICK;

    // ---- Phase A: vendor style, one CTLR store.
    stop_and_clear();
    stk.ctlr().write(|w| {
        w.set_streload(true);
        w.set_stclk(true);
        w.set_ste(true);
    });
    let (h, l) = cnt();
    scratch(1, l);
    scratch(2, h);
    scratch(3, stk.ctlr().read().0 as u32);

    for k in 0..8 {
        spin(2_000);
        let (h, l) = cnt();
        scratch(4 + k * 2, l);
        scratch(5 + k * 2, h);
        if stk.cntfg().read().cntif() && scratch_word(20) == 0 {
            scratch(20, 1);
            scratch(21, l);
            scratch(22, h);
        }
    }
    scratch(23, stk.ctlr().read().0 as u32);

    // ---- Phase B: driver style, CMP first, then a separate reload strobe.
    stop_and_clear();
    stk.ctlr().modify(|w| {
        w.set_stclk(true);
        w.set_ste(true);
    });
    stk.ctlr().modify(|w| w.set_streload(true));
    let (h, l) = cnt();
    scratch(8, l);
    scratch(9, h);
    spin(2_000);
    let (h, l) = cnt();
    scratch(10, l);
    scratch(11, h);
    scratch(12, stk.ctlr().read().0 as u32);

    loop {
        core::hint::spin_loop();
    }
}

fn scratch_word(i: usize) -> u32 {
    unsafe { core::ptr::read_volatile(SCRATCH.add(i)) }
}
