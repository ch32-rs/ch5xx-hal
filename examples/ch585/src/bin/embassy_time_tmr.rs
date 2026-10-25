//! Embassy async timers on a TMR instance, alongside the blocking delay.
//!
//! ```console
//! cargo run --release --bin embassy_time_tmr --no-default-features --features time-driver-tmr0
//! ```
//!
//! This is the reason to pick a `time-driver-tmr*` feature over the default
//! `time-driver-systick`: a TMR is an ordinary peripheral, so the core SysTick
//! keeps running as [`hal::delay`]'s free-running counter and both kinds of
//! delay work in the same program. Under the SysTick driver the time driver
//! reprograms SysTick (it needs `STRE = 1` and its own compare value), which
//! takes it away from `delay` entirely.
//!
//! The two are shown together but not mixed inside one measurement: a blocking
//! `delay_ms` stops the executor, so it would show up as extra latency on the
//! async timer.

#![no_std]
#![no_main]

// This example is about a TMR driver keeping `delay` alive, so a SysTick driver
// would defeat the point (and `Delay` would not even exist).
#[cfg(not(any(
    feature = "time-driver-tmr0",
    feature = "time-driver-tmr1",
    feature = "time-driver-tmr2",
    feature = "time-driver-tmr3"
)))]
compile_error!(
    "this example needs a TMR driver: cargo run --release --bin embassy_time_tmr --no-default-features --features time-driver-tmr0"
);

use ch5xx_hal as hal;
use ch5xx_hal::delay::Delay;
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
    let mut delay = Delay;
    loop {
        let start = Instant::now();
        Timer::after(Duration::from_millis(500)).await;
        info!("async timer elapsed {} ms", start.elapsed().as_millis());

        // SysTick is still ours, so the blocking delay is exact as well.
        let start = Instant::now();
        delay.delay_ms(100);
        info!("blocking delay took {} ms", start.elapsed().as_millis());
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    match hal::embassy::time_driver_resource() {
        hal::embassy::DriverResource::Tmr(irq) => info!("time driver: TMR ({=u8})", irq as u8),
        hal::embassy::DriverResource::SysTick => {
            info!("time driver: SysTick — `delay` is unavailable under this driver")
        }
    }
    info!("embassy sysclk = {} Hz", hal::rcc::clocks().sysclk.0);

    let led0 = Output::new(p.PB16, Level::Low, Speed::Low);
    let led1 = Output::new(p.PB17, Level::High, Speed::Low);

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
