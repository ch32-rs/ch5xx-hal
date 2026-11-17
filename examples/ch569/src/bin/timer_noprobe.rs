//! Drive the time driver's alarm with a raw waker, no executor.
//!
//! Arms a 100 ms wake, then watches the SysTick flag, the reload registers and
//! a wake counter. Separates "the alarm fires" from "the executor wakes".
//!
//! ```console
//! cargo build --release --bin timer_noprobe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 12
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn word(i: usize) -> u32 {
    unsafe { core::ptr::read_volatile(SCRATCH.add(i)) }
}

fn bump(i: usize) {
    scratch(i, word(i).wrapping_add(1));
}

fn raw_waker() -> core::task::Waker {
    unsafe fn clone(_: *const ()) -> core::task::RawWaker {
        core::task::RawWaker::new(core::ptr::null(), &VTABLE)
    }
    unsafe fn wake(_: *const ()) {
        bump(8);
    }
    unsafe fn wake_by_ref(_: *const ()) {
        bump(8);
    }
    unsafe fn drop(_: *const ()) {}

    static VTABLE: core::task::RawWakerVTable =
        core::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    unsafe { core::task::Waker::from_raw(core::task::RawWaker::new(core::ptr::null(), &VTABLE)) }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x5453_4550); // "TSEP"
    for i in 1..12 {
        scratch(i, 0);
    }

    let stk = &pac::SYSTICK;
    scratch(1, stk.cmplr().read().cmpl());
    scratch(2, stk.cmphr().read().cmph());

    let t0 = embassy_time_driver::now();
    scratch(3, t0 as u32);
    scratch(4, (t0 >> 32) as u32);

    let at = t0 + 100_000;
    scratch(5, at as u32);
    scratch(6, (at >> 32) as u32);

    let waker = raw_waker();
    embassy_time_driver::schedule_wake(at, &waker);

    // What did `arm` load?
    scratch(7, stk.cmplr().read().cmpl());
    scratch(9, stk.cmphr().read().cmph());
    scratch(10, stk.ctlr().read().0 as u32);

    loop {
        // First observation of the zero-crossing flag, then what the ISR left.
        if stk.cntfg().read().cntif() && word(11) == 0 {
            scratch(11, 1);
            scratch(2, stk.cmplr().read().cmpl());
            scratch(1, stk.cmphr().read().cmph());
        }
        if word(8) != 0 {
            scratch(2, stk.cmplr().read().cmpl());
            scratch(3, embassy_time_driver::now() as u32);
            scratch(4, (embassy_time_driver::now() >> 32) as u32);
        }
    }
}
