//! Set the CH582 real-time clock and read it back.
//!
//! No external signal pins are used. The RTC retains its value across software
//! resets, but this example overwrites it with a fixed date. It reads the calendar
//! four times, then checks a one-second alarm and interval timer. Timing depends
//! on the selected 32 kHz oscillator and its accuracy.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::delay::Delay;
use hal::rtc::{DateTime, Period, Rtc};
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut rtc = Rtc::new(p.RTC);
    let mut delay = Delay;

    let start = DateTime {
        year: 2026,
        month: 11,
        day: 8,
        hour: 12,
        minute: 0,
        second: 0,
    };
    rtc.set_datetime(start).unwrap();

    defmt::info!("RTC calendar");
    for _ in 0..4 {
        let now = rtc.datetime();
        defmt::info!(
            "{}-{}-{} {}:{}:{}",
            now.year,
            now.month,
            now.day,
            now.hour,
            now.minute,
            now.second
        );
        delay.delay_ms(1000);
    }

    rtc.stop_timer();
    rtc.clear_timer();
    rtc.clear_trigger();
    rtc.set_alarm_in(32_768);
    rtc.start_timer(Period::S1);
    delay.delay_ms(1100);
    defmt::info!("alarm {} timer {}", rtc.trigger_pending(), rtc.timer_pending());

    loop {
        core::hint::spin_loop();
    }
}
