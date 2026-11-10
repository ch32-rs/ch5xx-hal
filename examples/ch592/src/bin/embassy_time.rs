//! Embassy async ticker over defmt/RTT.
//!
//! The time driver is selected by the crate features: SysTick by default, or
//! `--no-default-features --features time-driver-tmr1` for the timer.
//!
//! ```console
//! PROBE_RS_PROBE=1a86:8010:F93D8F067E10 cargo run --release --bin embassy_time
//! PROBE_RS_PROBE=1a86:8010:F93D8F067E10 \
//!   cargo run --release --bin embassy_time --no-default-features --features time-driver-tmr1
//! ```

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
compile_error!("pick a time driver with one of the `time-driver-*` features");

use ch5xx_hal as hal;
use defmt::{error, info};
use defmt_rtt as _;
use embassy_time::{Duration, Instant, Timer};
use panic_halt as _;
use qingke_rt::entry;

#[embassy_executor::task]
async fn ticker() {
    let mut count = 0u32;
    let mut worst = 0u64;
    loop {
        let start = Instant::now();
        Timer::after(Duration::from_millis(500)).await;
        let elapsed = start.elapsed().as_millis();
        worst = worst.max(elapsed);
        info!("tick {} at {} ms (waited {})", count, Instant::now().as_millis(), elapsed);
        count = count.wrapping_add(1);

        // Ten ticks is enough to catch a wrong timebase scale; the residual
        // over 500 ms is interrupt latency and logging.
        if count == 10 {
            if (450..=550).contains(&worst) {
                info!("worst tick {} ms, SELFTEST PASS", worst);
            } else {
                error!("worst tick {} ms, SELFTEST FAIL", worst);
            }
        }
    }
}

#[entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(ticker().unwrap());
    })
}
