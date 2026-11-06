//! Exercise the window watchdog without letting it reset the board.
//!
//! Nothing is wired. Reset on overflow stays disabled, so the counter is free
//! to wrap. The program feeds it once, waits, and logs whether the overflow
//! flag came up. It also logs the latched reset cause and one battery-detector
//! sample. A USB-powered board should not report low.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::delay::Delay;
use hal::watchdog::{reset_cause, Battery, Level, ResetCause, Watchdog};
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = Delay;

    match reset_cause() {
        ResetCause::Software => defmt::info!("reset cause software"),
        ResetCause::PowerOn => defmt::info!("reset cause power-on"),
        ResetCause::Watchdog => defmt::info!("reset cause watchdog"),
        ResetCause::Pin => defmt::info!("reset cause pin"),
        ResetCause::ShutdownWake => defmt::info!("reset cause shutdown wake"),
        ResetCause::Other(flag) => defmt::info!("reset cause other {}", flag),
    }

    let battery = Battery::new(p.SYS, Level::V2_3);
    let status = battery.status();
    defmt::info!("battery low {} lower {}", status.low, status.lower);

    // At the 5.333 MHz reset clock the counter ticks about every 24.6 ms, so a
    // reload of 128 overflows in roughly 3.1 s.
    let watchdog = Watchdog::new(p.SYSTEM_CONTROL, 128);
    defmt::info!("overflow just after feed: {}", watchdog.overflowed());
    delay.delay_ms(4000);
    defmt::info!("overflow after four seconds: {}", watchdog.overflowed());
    watchdog.feed(128);
    defmt::info!("overflow after a second feed: {}", watchdog.overflowed());

    loop {
        core::hint::spin_loop();
    }
}
