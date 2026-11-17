//! Does a sleeping (`wfi`/`wfe`) core on CH569 wake on a SysTick interrupt?
//!
//! The embassy executor idles in `wfi`; with `time-driver-systick` the alarm
//! was pending but never serviced. This arms a 1 ms SysTick and sleeps, with
//! the same SCTLR `SEVONPEND` write the HAL's `enable_interrupts` does.
//!
//! ```console
//! cargo build --release --no-default-features --bin wfe_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 3
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use panic_halt as _;
use qingke::pfic;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const PERIOD: u32 = 15_000;

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
        let hits = core::ptr::read_volatile(SCRATCH.add(1));
        core::ptr::write_volatile(SCRATCH.add(1), hits.wrapping_add(1));
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    let stk = &pac::SYSTICK;
    unsafe {
        core::ptr::write_volatile(SCRATCH, 0x5746_4557); // "WEFW"
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

    // Same as the HAL's `enable_interrupts`: SEVONPEND then MIE.
    let sctlr = pac::PFIC.sctlr().read();
    let mut new = sctlr;
    new.set_sevonpend(true);
    pac::PFIC.sctlr().write_value(new);

    unsafe {
        pfic::set_priority(qingke_rt::CoreInterrupt::SysTick as u8, 0x80);
        pfic::enable_interrupt(qingke_rt::CoreInterrupt::SysTick as u8);
        core::arch::asm!("csrs mstatus, {}", const 0x8);
    }

    let mut n = 0u32;
    loop {
        // Same shape as the executor idle: wfi inside a critical section.
        unsafe {
            core::arch::asm!("csrci mstatus, 0x8", "wfi", "csrsi mstatus, 0x8", options(nomem, nostack));
        }
        n = n.wrapping_add(1);
        unsafe { core::ptr::write_volatile(SCRATCH.add(2), n) };
    }
}
