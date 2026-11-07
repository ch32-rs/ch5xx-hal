//! Closed-loop check of the GPIO port-interrupt edge waits.
//!
//! Short PA14 (output) to PA15 (input). No button is involved and no RTT input
//! is needed, so the whole run is automatic. The driver task produces a 5 Hz
//! square wave on PA14; the watcher awaits rising and falling edges on PA15 and
//! mirrors the level on LED0. After 20 cycles the driver prints the two counts:
//!
//! ```text
//! SELFTEST PASS rising=20 falling=20
//! SELFTEST FAIL rising=.. falling=.. expected=20
//! ```
//!
//! Both pins are on port A, where every PA0-PA15 has its own flag bit. The
//! PB22/PB23 alias through `PIN_INTX` is a separate path and is not covered
//! here. PA14/PA15 are the alternate SWD pins, but only when
//! `PIN_CONFIG.RB_DEBUG_PIN_SEL` is set; the reset value keeps SWD on PA8/PA9.
//!
//! Timing: the watcher arms within microseconds while each edge is held for
//! 100 ms, so a lost count means the interrupt did not arrive, not a race.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use embassy_time::Timer;
use hal::gpio::{Input, Level, Output, Pull, Speed};
use panic_halt as _;
use portable_atomic::{AtomicU32, Ordering};
use qingke_rt::entry;

const CYCLES: u32 = 20;
const HALF_MS: u64 = 100;

static RISING: AtomicU32 = AtomicU32::new(0);
static FALLING: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
async fn watcher(mut pin: Input<'static>, mut led: Output<'static>) -> ! {
    loop {
        pin.wait_for_rising_edge().await;
        RISING.fetch_add(1, Ordering::Relaxed);
        led.set_high();
        pin.wait_for_falling_edge().await;
        FALLING.fetch_add(1, Ordering::Relaxed);
        led.set_low();
    }
}

#[embassy_executor::task]
async fn driver(mut out: Output<'static>) -> ! {
    // Let the watcher's first wait arm before the first edge.
    Timer::after_millis(200).await;
    for _ in 0..CYCLES {
        out.set_high();
        Timer::after_millis(HALF_MS).await;
        out.set_low();
        Timer::after_millis(HALF_MS).await;
    }
    Timer::after_millis(HALF_MS).await;

    let rising = RISING.load(Ordering::Relaxed);
    let falling = FALLING.load(Ordering::Relaxed);
    if rising == CYCLES && falling == CYCLES {
        defmt::info!("SELFTEST PASS rising={} falling={}", rising, falling);
    } else {
        defmt::info!(
            "SELFTEST FAIL rising={} falling={} expected={}",
            rising,
            falling,
            CYCLES
        );
    }

    loop {
        Timer::after_millis(1000).await;
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let out = Output::new(p.PA14, Level::Low, Speed::Low);
    let input = Input::new(p.PA15, Pull::None);
    let led = Output::new(p.PB16, Level::Low, Speed::Low);
    defmt::info!("PA14 -> PA15 loopback, 20 cycles");

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(watcher(input, led).unwrap());
        spawner.spawn(driver(out).unwrap());
    })
}
