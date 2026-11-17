//! Does the CH569 SysTick core interrupt fire under the driver's reload model?
//!
//! Rearms CMP + STRELOAD from the handler, exactly like
//! `time_driver_systick_ch573`'s `arm`, and counts hits in RAMX. Build without
//! a time driver so this handler owns the SysTick vector.
//!
//! ```console
//! cargo build --release --no-default-features --bin systick_irq
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 4
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use panic_halt as _;
use qingke::pfic;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

/// 1 ms at 15 MHz.
const PERIOD: u32 = 15_000;

fn rearm() {
    pac::SYSTICK.cmplr().write(|w| w.set_cmpl(PERIOD));
    pac::SYSTICK.cmphr().write(|w| w.set_cmph(0));
    pac::SYSTICK.ctlr().modify(|w| w.set_streload(true));
}

#[qingke_rt::interrupt(core)]
fn SysTick() {
    unsafe {
        let hits = core::ptr::read_volatile(SCRATCH.add(1));
        core::ptr::write_volatile(SCRATCH.add(1), hits.wrapping_add(1));
    }
    if !pac::SYSTICK.cntfg().read().cntif() {
        unsafe { core::ptr::write_volatile(SCRATCH.add(2), 0xdead_0001) };
        return;
    }
    pac::SYSTICK.cntfg().modify(|w| w.set_cntif(false));
    rearm();
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    let stk = &pac::SYSTICK;
    unsafe {
        core::ptr::write_volatile(SCRATCH, 0x4954_5359); // "YSTI"
        core::ptr::write_volatile(SCRATCH.add(1), 0);
        core::ptr::write_volatile(SCRATCH.add(2), 0);
    }

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
        core::arch::asm!("csrs mstatus, {}", const 0x8);
    }

    loop {
        unsafe {
            core::ptr::write_volatile(SCRATCH.add(3), stk.cntl().read().cntl());
        }
    }
}
