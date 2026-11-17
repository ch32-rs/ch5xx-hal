//! Is the embassy timebase sane on CH569, without any executor?
//!
//! Reads the configured clock, two timestamps around a busy loop, and the
//! SysTick registers. `now()` is the driver's raw tick count.
//!
//! ```console
//! cargo build --release --bin timebase_probe
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

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x5442_5345); // "TBSE"
    for i in 1..12 {
        scratch(i, 0);
    }

    scratch(1, hal::rcc::clocks().sysclk.0);
    scratch(2, hal::delay::measured_sysclk().0);

    let t0 = embassy_time_driver::now();
    scratch(3, t0 as u32);
    scratch(4, (t0 >> 32) as u32);

    // Unknown work: a plain counting loop, then sample again.
    let mut n = 0u32;
    while n < 200_000 {
        n = n.wrapping_add(1);
        core::hint::spin_loop();
    }
    scratch(5, n);

    let t1 = embassy_time_driver::now();
    scratch(6, t1 as u32);
    scratch(7, (t1 >> 32) as u32);
    scratch(8, t1.wrapping_sub(t0) as u32);

    let stk = &pac::SYSTICK;
    scratch(9, stk.cmplr().read().cmpl());
    scratch(10, stk.cmphr().read().cmph());
    scratch(11, stk.ctlr().read().0 as u32);

    // Keep ticking so a later read can tell live memory from a stale image.
    loop {
        let mut m = 0u32;
        while m < 100_000 {
            m = m.wrapping_add(1);
        }
        let t = embassy_time_driver::now();
        scratch(2, hal::delay::measured_sysclk().0);
        word(1);
        scratch(0, 0x5442_5345);
        scratch(1, hal::rcc::clocks().sysclk.0);
        scratch(8, t.wrapping_sub(t0) as u32);
    }
}
