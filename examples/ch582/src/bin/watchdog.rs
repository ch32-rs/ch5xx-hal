//! CH582 watchdog reset and overflow self-test, without external wiring.
//!
//! A non-watchdog boot arms a reset. After that reset, the test checks that
//! feeding clears the overflow flag and that another overflow sets it. An
//! existing watchdog reset cause also enters the second phase; power-cycle or
//! reset the board to start the complete test again.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::delay::Delay;
use hal::watchdog::{reset_cause, ResetCause, Watchdog};
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = Delay;
    let cause = reset_cause();

    if cause != ResetCause::Watchdog {
        defmt::info!("arming watchdog reset");
        let watchdog = Watchdog::new(p.SYSTEM_CONTROL, 0);
        watchdog.enable_reset();
        // At the default 6.4 MHz Fsys, overflow takes about 5.24 seconds.
        delay.delay_ms(8000);
        watchdog.disable_reset();
        defmt::error!("SELFTEST FAIL: watchdog reset did not occur");
    } else {
        defmt::info!("watchdog reset cause confirmed");
        // new() feeds the watchdog and clears any pre-existing overflow flag.
        let watchdog = Watchdog::new(p.SYSTEM_CONTROL, 128);
        watchdog.disable_reset();
        let clear_after_feed = !watchdog.overflowed();
        // At 6.4 MHz, 128 ticks take about 2.62 seconds.
        delay.delay_ms(5000);
        let set_after_overflow = watchdog.overflowed();
        watchdog.feed(128);
        let clear_after_refeed = !watchdog.overflowed();

        if clear_after_feed && set_after_overflow && clear_after_refeed {
            defmt::info!("SELFTEST PASS: watchdog reset and overflow flag");
        } else {
            defmt::error!(
                "SELFTEST FAIL: clear after feed {}, set after overflow {}, clear after refeed {}",
                clear_after_feed,
                set_after_overflow,
                clear_after_refeed
            );
        }
    }

    loop {
        core::hint::spin_loop();
    }
}
