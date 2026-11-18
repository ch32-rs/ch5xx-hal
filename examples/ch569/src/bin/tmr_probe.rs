//! TMR0/TMR1/TMR2 counters on CH569, checked against embassy time.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::timer::low_level::Timer;
use ch5xx_hal::timer::{Events, PeriodTicks, MAX_PERIOD};
use embassy_time::Instant;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const MEASURE_MS: u32 = 100;
const TOLERANCE_PCT: u64 = 5;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

/// Returns 1 when the measured rate and the cycle-end event both hold.
fn check<T: hal::timer::Instance>(peri: hal::Peri<'_, T>, sysclk: u32, slot: usize) -> u32 {
    let mut timer = Timer::new(peri);
    let mut delay = hal::delay::Delay;

    timer.configure_counter(PeriodTicks::new(MAX_PERIOD).unwrap());
    timer.start();
    let t0 = Instant::now();
    let start = timer.count();
    delay.delay_ms(MEASURE_MS);
    let ticks = timer.count().wrapping_sub(start);
    let elapsed_us = t0.elapsed().as_micros();

    let expected = (elapsed_us * sysclk as u64 / 1_000_000) as u32;
    let rate_ok = expected > 0 && ticks.abs_diff(expected) as u64 * 100 <= expected as u64 * TOLERANCE_PCT;

    let khz = PeriodTicks::from_hz(sysclk, 1000).unwrap();
    timer.configure_counter(khz);
    timer.start();
    delay.delay_ms(10);
    let cycle_ok = timer.events().contains(Events::CYCLE_END);
    timer.acknowledge(Events::ALL);

    scratch(slot * 4 + 2, ticks);
    scratch(slot * 4 + 3, expected);
    scratch(slot * 4 + 4, cycle_ok as u32);
    (rate_ok && cycle_ok) as u32
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let sysclk = hal::rcc::clocks().sysclk.0;

    scratch(0, 0x544d_5250); // "TMRP"
    scratch(1, sysclk);
    scratch(2, 0);
    scratch(3, 0);

    let mut ok = 0;
    ok += check(p.TMR0, sysclk, 1);
    ok += check(p.TMR1, sysclk, 2);
    ok += check(p.TMR2, sysclk, 3);

    scratch(1, sysclk);
    scratch(2, ok);
    scratch(3, if ok == 3 { 0x5041_5353 } else { 0x4641_494c }); // "PASS" / "FAIL"

    loop {
        core::hint::spin_loop();
    }
}
