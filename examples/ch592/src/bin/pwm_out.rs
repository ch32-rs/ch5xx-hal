//! Generate a PWM signal on CH592X and measure it back with a GPIO input.
//!
//! PWM8 leaves on PB6 and the check samples PB5, so both channels only need one
//! jumper: PB6 to PB5 on the P9/P7 header. The input has a pull-down, and the
//! channel runs active high, so the idle level is low and every rising edge is
//! one PWM period.
//!
//! The block runs from the 6.4 MHz default clock with divisor 64 and a 256-clock
//! cycle, which is 390.6 Hz. The 32 kHz RTC measures one second of edges while
//! the loop also samples the level, so both the period and the 50% duty of the
//! `0x80` compare value are checked.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::{error, info};
use defmt_rtt as _;
use hal::gpio::{Input, Pull};
use hal::pwm::{Config, Cycle, Polarity, Pwm};
use hal::rtc::Rtc;
use panic_halt as _;
use qingke_rt::entry;

/// PWM clock divisor.
const DIVISOR: u8 = 64;
/// Compare value for the 256-clock cycle, about half duty.
const DUTY: u16 = 0x80;
/// One second of the 32 kHz RTC counter.
const WINDOW: u32 = 32_768;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let sysclk = hal::rcc::clocks().sysclk.0;

    let mut pwm = match Pwm::new(p.PWMX, Config::new(DIVISOR, Cycle::Clocks256)) {
        Ok(pwm) => pwm,
        Err(_) => {
            error!("PWM0 init rejected the divisor");
            loop {
                core::hint::spin_loop();
            }
        }
    };
    let mut channel = pwm.channel(p.PB6, Polarity::ActiveHigh);
    channel.set_duty(DUTY);

    // Read the byte back through the PAC: on CH592 each channel has its own
    // 8-bit register, and this proves the write landed where the waveform
    // should come from even if the jumper is missing.
    let readback = hal::pac::PWMX.pwm8_data().read().pwm8_data();
    let out_enabled = hal::pac::PWMX.out_en().read().pwm8_out_en();
    if readback != DUTY as u8 || !out_enabled {
        error!("PWM8 register readback {} and output {} wrong", readback, out_enabled);
        loop {
            core::hint::spin_loop();
        }
    }

    let probe = Input::new(p.PB5, Pull::Down);
    let rtc = Rtc::new(p.RTC);

    info!("PWM8 on PB6, jumper PB6 to PB5");

    // Sample PB5 for one second of RTC time and count periods and high samples.
    let start = rtc.cycle_count();
    let mut edges = 0u32;
    let mut high = 0u32;
    let mut samples = 0u32;
    let mut last = probe.is_high();
    loop {
        let level = probe.is_high();
        if level && !last {
            edges += 1;
        }
        last = level;
        if level {
            high += 1;
        }
        samples += 1;
        if samples % 256 == 0 && rtc.cycle_count().wrapping_sub(start) >= WINDOW {
            break;
        }
    }

    let expected = (sysclk as u64 / DIVISOR as u64 / Cycle::Clocks256.clocks() as u64) as u32;
    let duty_pct = high as u64 * 100 / samples.max(1) as u64;
    let expected_duty = u64::from(DUTY) * 100 / 255;
    let freq_ok = edges.abs_diff(expected) as u64 * 100 <= expected as u64 * 5;
    let duty_ok = duty_pct.abs_diff(expected_duty) <= 5;

    if freq_ok && duty_ok {
        info!(
            "SELFTEST PASS: {} edges in 1 s (expected {}), duty {}% (expected {}%)",
            edges, expected, duty_pct, expected_duty
        );
    } else if edges == 0 {
        error!(
            "SELFTEST FAIL: no edges on PB5; jumper PB6 to PB5 (duty register reads {})",
            readback
        );
    } else {
        error!(
            "SELFTEST FAIL: {} edges (expected {}), duty {}% (expected {}%)",
            edges, expected, duty_pct, expected_duty
        );
    }

    loop {
        core::hint::spin_loop();
    }
}
