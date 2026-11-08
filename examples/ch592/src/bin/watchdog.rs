//! Exercise the CH592X window watchdog, including a real overflow reset.
//!
//! Nothing is wired. The first boot enables reset on overflow and stops
//! feeding, so the chip resets after the counter wraps. The boot after that
//! sees a watchdog reset cause, disables reset, and checks the feed path: the
//! overflow flag must be clear just after a feed, set after the counter wraps,
//! and clear again after the next feed. It also logs one battery-detector
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
    let cause = reset_cause();

    match cause {
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

    if cause != ResetCause::Watchdog {
        // First boot: let an overflow reset the chip. `Watchdog::new` reloads
        // the counter; nothing feeds it afterwards.
        defmt::info!("arming watchdog reset");
        let watchdog = Watchdog::new(p.SYSTEM_CONTROL, 0);
        watchdog.enable_reset();
        loop {
            core::hint::spin_loop();
        }
    }

    defmt::info!("watchdog reset confirmed");
    // Second boot: keep reset disabled so the counter can wrap safely.
    // The counter ticks at `Fsys / 131072`; at the 6.4 MHz default clock that
    // is about 49 Hz, so a reload of 128 overflows in roughly 2.6 s.
    let watchdog = Watchdog::new(p.SYSTEM_CONTROL, 128);
    watchdog.disable_reset();
    defmt::info!("overflow just after feed: {}", watchdog.overflowed());
    delay.delay_ms(5000);
    defmt::info!("overflow after five seconds: {}", watchdog.overflowed());
    watchdog.feed(128);
    defmt::info!("overflow after a second feed: {}", watchdog.overflowed());

    loop {
        core::hint::spin_loop();
    }
}
