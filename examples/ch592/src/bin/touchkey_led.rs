//! Touch buttons driving an LED.
//!
//! K1 (PA6) and K2 (PA7) are the board's touch pads and LED2 is wired to PA4.
//! The board routes the LED through header P9, so short pin 14 (LED2) to pin 13
//! (PA4). The LED sits between 3V3 and its 1 kOhm series resistor, so PA4 low
//! lights it: the example drives the pin high for off.
//!
//! The parameters follow the vendor touch library's working point: two charge
//! clocks and three discharge clocks at 70 µA (`EVT/EXAM/TOUCH/TKYLIB`, the
//! `TouchKey_CFG.h` channels all use charge 2, discharge 3), untouched counts
//! around 3200..3800, and thresholds in the vendor's 71..103 range.
//!
//! The baseline follows the reading continuously with a time constant of 256
//! readings, the vendor's slow base refresh. Following it only while released
//! would deadlock: a hand near the pads drags the baseline down, and the step
//! when it leaves freezes the follower above the release threshold.
//!
//! A press is a deviation of at least [`TOUCH_DELTA`] counts that lasts
//! [`PRESS_READINGS`] readings; the deviation has to fall back under
//! [`RELEASE_DELTA`] before another press is accepted, so holding a finger
//! down does not repeat. A touch on either pad toggles the LED: the two pads
//! are close enough that touching one couples into the other.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::{error, info};
use defmt_rtt as _;
use hal::delay::Delay;
use hal::gpio::{Level, Output, Speed};
use hal::touchkey::{Config, Pad, TouchKey};
use panic_halt as _;
use qingke_rt::entry;

/// Acquisitions averaged into one reading.
const AVERAGE: u32 = 8;
/// Readings in the untouched baseline.
const BASELINE_READINGS: u32 = 40;
/// Pause between readings, in milliseconds.
const READING_PERIOD_MS: u32 = 20;
/// Deviation from the baseline that counts as a touch, in ADC counts.
const TOUCH_DELTA: u16 = 100;
/// Deviation below which a pad counts as released, in ADC counts.
const RELEASE_DELTA: u16 = 60;
/// Consecutive readings above the threshold before a press is accepted.
const PRESS_READINGS: u32 = 2;
/// Consecutive released readings before another press is accepted.
const RELEASE_READINGS: u32 = 2;
/// Log the readings every this many samples.
const LOG_EVERY: u32 = 40;
/// The baseline is kept in 1/256 counts so it can follow fractions of a count.
const BASELINE_FRAC: u32 = 8;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = Delay;
    // LED2 is active low: 3V3 -> D4 -> R10 (1 kOhm) -> PA4.
    let mut led = Output::new(p.PA4, Level::High, Speed::Low);
    for _ in 0..3 {
        led.set_low();
        delay.delay_ms(120);
        led.set_high();
        delay.delay_ms(120);
    }

    let mut key = match TouchKey::new(p.TOUCHKEY, p.ADC, Config::default()) {
        Ok(key) => key,
        Err(_) => {
            error!("TouchKey::new rejected the configuration");
            loop {
                core::hint::spin_loop();
            }
        }
    };
    let mut k1 = Pad::new(p.PA6);
    let mut k2 = Pad::new(p.PA7);

    let mut baseline1 = fixed(baseline(&mut key, &mut k1, &mut delay));
    let mut baseline2 = fixed(baseline(&mut key, &mut k2, &mut delay));
    info!("baseline K1 {} K2 {}", baseline1 >> BASELINE_FRAC, baseline2 >> BASELINE_FRAC);
    info!("touch either pad to toggle LED2");

    let mut streak = 0u32;
    let mut released = 0u32;
    let mut armed = true;
    let mut tick = 0u32;
    loop {
        let value1 = read_average(&mut key, &mut k1);
        let value2 = read_average(&mut key, &mut k2);
        let delta1 = deviation(baseline1, value1);
        let delta2 = deviation(baseline2, value2);
        let delta = delta1.max(delta2);

        baseline1 = follow(baseline1, value1);
        baseline2 = follow(baseline2, value2);
        streak = if delta >= TOUCH_DELTA { streak + 1 } else { 0 };
        released = if delta <= RELEASE_DELTA {
            released + 1
        } else {
            0
        };
        if released >= RELEASE_READINGS {
            armed = true;
        }
        if armed && streak >= PRESS_READINGS {
            armed = false;
            led.toggle();
            let pad = if delta1 >= delta2 { "K1" } else { "K2" };
            let state = if led.is_set_low() { "on" } else { "off" };
            info!("{} touched (delta {}), LED2 {}", pad, delta, state);
        }
        if tick.is_multiple_of(LOG_EVERY) {
            info!("K1 {} (delta {}) K2 {} (delta {})", value1, delta1, value2, delta2);
        }
        tick += 1;
        delay.delay_ms(READING_PERIOD_MS);
    }
}

/// A reading in 1/256 count units.
fn fixed(value: u16) -> i32 {
    i32::from(value) << BASELINE_FRAC
}

/// Deviation of `value` from `baseline`, in counts.
fn deviation(baseline: i32, value: u16) -> u16 {
    ((fixed(value) - baseline).unsigned_abs() >> BASELINE_FRAC) as u16
}

/// Follow the reading with a time constant of 256 readings: about six seconds
/// here, far slower than a touch but fast enough to absorb drift.
fn follow(baseline: i32, value: u16) -> i32 {
    baseline + (fixed(value) - baseline) / 256
}

/// Average one reading of `pad`, dropping the first acquisition after the
/// channel change.
fn read_average(key: &mut TouchKey<'_>, pad: &mut Pad<'_>) -> u16 {
    let mut sum = 0u32;
    for acquisition in 0..=AVERAGE {
        let value = key.read(pad).unwrap_or(0);
        if acquisition > 0 {
            sum += u32::from(value);
        }
    }
    (sum / AVERAGE) as u16
}

/// Average [`BASELINE_READINGS`] readings of `pad`.
fn baseline(key: &mut TouchKey<'_>, pad: &mut Pad<'_>, delay: &mut Delay) -> u16 {
    let mut sum = 0u32;
    for _ in 0..BASELINE_READINGS {
        sum += u32::from(read_average(key, pad));
        delay.delay_ms(READING_PERIOD_MS);
    }
    (sum / BASELINE_READINGS) as u16
}
