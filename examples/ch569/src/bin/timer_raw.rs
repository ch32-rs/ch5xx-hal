//! Does the SysTick time driver's `schedule_wake` callback fire without the
//! embassy executor? Busy-waits, so no executor or sleep is involved.
//!
//! ```console
//! cargo build --release --bin timer_raw
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 6
//! ```

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

static CALLBACKS: AtomicU32 = AtomicU32::new(0);

// A waker that only bumps the callback count, so the queue still runs.
const VTABLE: core::task::RawWakerVTable =
    core::task::RawWakerVTable::new(|_| RAW, |_| {}, |_| {}, |_| {});
const RAW: core::task::RawWaker = core::task::RawWaker::new(core::ptr::null(), &VTABLE);

fn on_alarm() {
    CALLBACKS.fetch_add(1, Ordering::Relaxed);
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x5452_4157); // "TRAW"
    let mut n = 0u32;
    loop {
        let fired = CALLBACKS.load(Ordering::Relaxed);
        let now = embassy_time_driver::now();
        scratch(4, now as u32);
        let waker = unsafe { core::task::Waker::from_raw(RAW) };
        embassy_time_driver::schedule_wake(now + 1_000, &waker); // 1 ms
        scratch(5, embassy_time_driver::now() as u32);

        // Give the interrupt 10 ms of wall clock in embassy ticks (1 MHz).
        let start = embassy_time_driver::now();
        while embassy_time_driver::now() - start < 10_000 {}

        n += 1;
        scratch(1, n);
        scratch(2, CALLBACKS.load(Ordering::Relaxed));
        if CALLBACKS.load(Ordering::Relaxed) == fired {
            scratch(3, n); // first iteration whose callback did not run
        }
    }
}
