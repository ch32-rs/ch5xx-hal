//! Touch keys on CH592X.
//!
//! PA6 and PA7 are the board's two touch pads (ADC channels 10 and 11). The
//! example averages eight charge-transfer acquisitions into one reading,
//! records a two-second untouched baseline, then reports the largest deviation
//! for sixty seconds. Touch each pad during that window: a deviation of at
//! least [`TOUCH_DELTA`] counts as a press, and the run ends with
//! `SELFTEST PASS` when both pads were seen.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::{error, info};
use defmt_rtt as _;
use hal::delay::Delay;
use hal::touchkey::{Config, Pad, TouchKey};
use panic_halt as _;
use qingke_rt::entry;

/// Acquisitions averaged into one reading.
const AVERAGE: u32 = 8;
/// Readings in the untouched baseline.
const BASELINE_READINGS: u32 = 40;
/// Pause between readings, in milliseconds.
const READING_PERIOD_MS: u32 = 50;
/// Readings in the window where a touch is expected.
const WINDOW_READINGS: u32 = 1200;
/// Deviation from the baseline that counts as a touch, in ADC counts.
const TOUCH_DELTA: u16 = 100;
/// Consecutive readings above the threshold before a pad counts as pressed.
const PRESS_READINGS: u32 = 3;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = Delay;
    let mut key = match TouchKey::new(p.TOUCHKEY, p.ADC, Config::default()) {
        Ok(key) => key,
        Err(_) => {
            error!("TouchKey::new rejected the configuration");
            loop {
                core::hint::spin_loop();
            }
        }
    };
    let mut pa6 = Pad::new(p.PA6);
    let mut pa7 = Pad::new(p.PA7);

    info!("touch PA6 and PA7 after the baseline");
    let baseline6 = baseline(&mut key, &mut pa6, &mut delay);
    let baseline7 = baseline(&mut key, &mut pa7, &mut delay);
    info!("baseline PA6 {} PA7 {}", baseline6, baseline7);

    let mut max6 = 0u16;
    let mut max7 = 0u16;
    let mut streak6 = 0u32;
    let mut streak7 = 0u32;
    let mut touched6 = false;
    let mut touched7 = false;
    for reading in 0..WINDOW_READINGS {
        let value6 = read_average(&mut key, &mut pa6);
        let value7 = read_average(&mut key, &mut pa7);
        let delta6 = value6.abs_diff(baseline6);
        let delta7 = value7.abs_diff(baseline7);
        max6 = max6.max(delta6);
        max7 = max7.max(delta7);
        streak6 = if delta6 >= TOUCH_DELTA { streak6 + 1 } else { 0 };
        streak7 = if delta7 >= TOUCH_DELTA { streak7 + 1 } else { 0 };
        if streak6 >= PRESS_READINGS && !touched6 {
            touched6 = true;
            info!("PA6 pressed: {} (baseline {})", value6, baseline6);
        }
        if streak7 >= PRESS_READINGS && !touched7 {
            touched7 = true;
            info!("PA7 pressed: {} (baseline {})", value7, baseline7);
        }
        if reading % 20 == 0 {
            info!("PA6 {} (delta {}) PA7 {} (delta {})", value6, delta6, value7, delta7);
        }
        delay.delay_ms(READING_PERIOD_MS);
    }

    if touched6 && touched7 {
        info!("SELFTEST PASS: PA6 max delta {} PA7 max delta {}", max6, max7);
    } else {
        error!("SELFTEST FAIL: PA6 max delta {} PA7 max delta {}", max6, max7);
    }

    loop {
        core::hint::spin_loop();
    }
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
