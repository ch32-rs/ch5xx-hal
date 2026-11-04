//! Embassy async blink, driven by the SysTick time driver.
//!
//! ```console
//! cargo run --release --bin embassy_time
//! ```
//!
//! Two tasks share the executor and the same time driver: the LED blinks every
//! 500 ms, and the log line prints how long the previous `Timer` actually took.
//! The measured interval should stay within a few tens of milliseconds of the
//! requested one; the residual is interrupt latency plus the cost of the
//! `println` itself, not clock drift.

#![no_std]
#![no_main]

// Without a driver the link fails on `_embassy_time_now`, which does not say
// what to do. This does.
#[cfg(not(any(
    feature = "time-driver-systick",
    feature = "time-driver-tmr0",
    feature = "time-driver-tmr1",
    feature = "time-driver-tmr2",
    feature = "time-driver-tmr3"
)))]
compile_error!(
    "pick a time driver: cargo run --release --bin embassy_time --no-default-features --features time-driver-systick"
);

use ch5xx_hal as hal;
use ch5xx_hal::gpio::{Level, Output, Speed};
use defmt::info;
use defmt_rtt as _;
use embassy_time::{Duration, Instant, Timer};
use panic_halt as _;
use qingke_rt::entry;

#[embassy_executor::task]
async fn blink(mut led0: Output<'static>, mut led1: Output<'static>) {
    let mut n: u32 = 0;
    loop {
        led0.toggle();
        led1.toggle();
        n += 1;
        info!("toggle {} at {} ms", n, Instant::now().as_millis());
        Timer::after(Duration::from_millis(500)).await;
    }
}

#[embassy_executor::task]
async fn measure() {
    loop {
        let start = Instant::now();
        Timer::after(Duration::from_millis(500)).await;
        info!("elapsed {} ms", start.elapsed().as_millis());
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("embassy sysclk = {} Hz", hal::rcc::clocks().sysclk.0);

    let led0 = Output::new(p.PB16, Level::Low, Speed::Low);
    let led1 = Output::new(p.PB17, Level::High, Speed::Low);

    // `Executor::new` is not `const`, so it must be built at runtime. This
    // function never returns, so it is safe to hand out a `'static` reference
    // to storage that lives for the rest of the program.
    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(blink(led0, led1).unwrap());
        spawner.spawn(measure().unwrap());
    })
}
