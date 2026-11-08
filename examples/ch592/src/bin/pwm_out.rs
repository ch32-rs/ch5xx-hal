//! Generate a PWM signal on CH592X and measure it back with a GPIO input.
//!
//! PWM5 leaves on PA13 and the check samples PA14, so both pins only need one
//! jumper: P9 pin 9 (PA13) to P9 pin 10 (PA14) on the base board, or P2 pin 10
//! to pin 8 on the core module. The input has a pull-down, and the channel runs
//! active high, so the idle level is low and every rising edge is one PWM
//! period.
//!
//! The block runs from the default 6.4 MHz clock with divisor 64 and a 256-clock
//! cycle: 64 x 256 = 16384 `Fsys` clocks per period, or 390.6 Hz. TMR2 also
//! counts `Fsys`, so the edge-to-edge span in its ticks is the period in clock
//! cycles and needs no external timebase.
//!
//! The period is not timed with the RTC because the 32 kHz domain resets to the
//! internal RC (`CK32K_CONFIG.RB_CLK_OSC32K_XT` is clear), not the board's
//! 32.768 kHz crystal. On this board the RC runs about ten percent fast, so an
//! "RTC second" is short and undercounts the edges; that is what made an
//! earlier version report 353 Hz. The duty is measured over forty periods, so
//! the sample count is high enough for a percentage.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::{error, info};
use defmt_rtt as _;
use hal::gpio::{Input, Pull};
use hal::pwm::{Config, Cycle, Polarity, Pwm};
use hal::timer::low_level::Timer;
use hal::timer::{PeriodTicks, MAX_PERIOD};
use panic_halt as _;
use qingke_rt::entry;

/// PWM clock divisor.
const DIVISOR: u8 = 64;
/// Compare value for the 256-clock cycle, about half duty.
const DUTY: u16 = 0x80;
/// Rising edges timestamped for the period check.
const EDGES: u32 = 11;
/// Periods sampled for the duty check.
const DUTY_PERIODS: u32 = 40;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let sysclk = hal::rcc::clocks().sysclk.0;

    let mut pwm = match Pwm::new(p.PWMX, Config::new(DIVISOR, Cycle::Clocks256)) {
        Ok(pwm) => pwm,
        Err(_) => {
            error!("Pwm::new rejected the divisor");
            loop {
                core::hint::spin_loop();
            }
        }
    };
    let mut channel = pwm.channel(p.PA13, Polarity::ActiveHigh);
    channel.set_duty(DUTY);

    // Read the byte back through the PAC: on CH592 each channel has its own
    // 8-bit register, and this proves the write landed where the waveform
    // should come from even if the jumper is missing.
    let readback = hal::pac::PWMX.pwm5_data().read().pwm5_data();
    let out_enabled = hal::pac::PWMX.out_en().read().pwm5_out_en();
    if readback != DUTY as u8 || !out_enabled {
        error!("PWM5 register readback {} and output {} wrong", readback, out_enabled);
        loop {
            core::hint::spin_loop();
        }
    }

    let probe = Input::new(p.PA14, Pull::Down);
    let mut timer = Timer::new(p.TMR2);
    timer.configure_counter(PeriodTicks::new(MAX_PERIOD).unwrap());
    timer.start();

    info!("PWM5 on PA13, jumper PA13 to PA14");

    // Timestamp [`EDGES`] rising edges; the span covers `EDGES - 1` periods.
    let mut edges = 0u32;
    let mut first = 0u32;
    let mut last = 0u32;
    let mut prev_high = probe.is_high();
    while edges < EDGES {
        let high = probe.is_high();
        if high && !prev_high {
            let now = timer.count();
            if edges == 0 {
                first = now;
            }
            last = now;
            edges += 1;
        }
        prev_high = high;
    }
    let span = last.wrapping_sub(first);
    let period = span / (EDGES - 1);
    let expected_period = u32::from(DIVISOR) * u32::from(Cycle::Clocks256.clocks());

    // Measure the duty over a fixed number of periods.
    let duty_start = timer.count();
    let mut high = 0u64;
    let mut samples = 0u64;
    while timer.count().wrapping_sub(duty_start) < expected_period * DUTY_PERIODS {
        if probe.is_high() {
            high += 1;
        }
        samples += 1;
    }
    let duty_pct = high * 100 / samples.max(1);
    let expected_duty = u64::from(DUTY) * 100 / 255;

    let period_ok = period.abs_diff(expected_period) as u64 * 100 <= u64::from(expected_period);
    let duty_ok = duty_pct.abs_diff(expected_duty) <= 5;
    let freq = sysclk as u64 / expected_period as u64;

    if period_ok && duty_ok {
        info!(
            "SELFTEST PASS: period {} Fsys ticks (expected {}), {} Hz, duty {}% (expected {}%)",
            period, expected_period, freq, duty_pct, expected_duty
        );
    } else if edges == 0 {
        error!(
            "SELFTEST FAIL: no edges on PA14; jumper PA13 to PA14 (duty register reads {})",
            readback
        );
    } else {
        error!(
            "SELFTEST FAIL: period {} Fsys ticks (expected {}), duty {}% (expected {}%)",
            period, expected_period, duty_pct, expected_duty
        );
    }

    loop {
        core::hint::spin_loop();
    }
}
