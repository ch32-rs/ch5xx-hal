//! Does `wfi` wake with `mstatus.MIE = 0` (the executor's idle shape)?

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use panic_halt as _;
use qingke::pfic;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const PERIOD: u32 = 12_000; // 100 us at 120 MHz

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
        let hits = core::ptr::read_volatile(SCRATCH.add(4));
        core::ptr::write_volatile(SCRATCH.add(4), hits.wrapping_add(1));
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x5746_4954); // "WFIT"
    for i in 1..8 {
        scratch(i, 0);
    }

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
    }

    // Phase A: MIE set, control.
    unsafe {
        core::arch::asm!("csrs mstatus, 0x8");
        core::arch::asm!("wfi");
    }
    scratch(1, 0xa000_0001);

    // Phase B: the executor's shape, WFI with MIE clear.
    #[cfg(feature = "wfi-mie0")]
    unsafe {
        // A real WFI: wakes on a pending enabled interrupt even with MIE = 0.
        let sctlr = pac::PFIC.sctlr().read();
        let mut new = sctlr;
        new.set_wfitowfe(false);
        new.set_sevonpend(true);
        pac::PFIC.sctlr().write_value(new);

        core::arch::asm!("csrci mstatus, 0x8");
        core::arch::asm!("wfi");
        core::arch::asm!("csrsi mstatus, 0x8");
        scratch(2, 0xb000_0002);
    }
    scratch(5, 0xdead_0001);

    loop {
        core::hint::spin_loop();
    }
}
