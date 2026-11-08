//! Generate a 16-bit-period PWM signal on CH592X and measure it back.
//!
//! PWM5 leaves on PA13 and the check samples PA14, so both pins only need one
//! jumper: P9 pin 9 (PA13) to P9 pin 10 (PA14) on the base board, or P2 pin 10
//! to pin 8 on the core module.
//!
//! [`Cycle::Clocks16`] selects the block's 16-bit counter mode: `CYC_MOD` is 3,
//! `REG_CYCLE` holds the period minus one, and the duty registers of PWM4 to
//! PWM9 become 16 bits wide. This example programs 16384 clocks with divisor 1,
//! so the period is 16384 `Fsys` clocks (390.6 Hz at the 6.4 MHz reset clock)
//! and the duty is 8192, an exact 50%. The 8-bit modes cannot express either
//! value for this period.
//!
//! PWM5's 16-bit duty occupies the two byte registers at 0x06 and 0x07, so the
//! example reads both back: the high byte proves the wide write path, which the
//! 8-bit modes never touch for this channel.

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
const DIVISOR: u8 = 1;
/// Period in divided clocks.
const PERIOD: u16 = 16384;
/// Duty in divided clocks, exactly half of [`PERIOD`].
const DUTY: u16 = PERIOD / 2;
/// Edge intervals collected for the period check. Odd, so the median is a
/// measured interval. The median ignores the outliers a defmt RTT write can
/// cause by stalling the core across an edge.
const PERIOD_SAMPLES: usize = 15;
/// Periods sampled for the duty check.
const DUTY_PERIODS: u32 = 40;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let sysclk = hal::rcc::clocks().sysclk.0;

    let pwm = match Pwm::new(p.PWMX, Config::new(DIVISOR, Cycle::Clocks16(PERIOD))) {
        Ok(pwm) => pwm,
        Err(_) => {
            error!("Pwm::new rejected the 16-bit cycle");
            loop {
                core::hint::spin_loop();
            }
        }
    };
    let mut channel = pwm.channel(p.PA13, Polarity::ActiveHigh);
    channel.set_duty(DUTY);

    // Read the wide registers back through the PAC. PWM5 is the second pair:
    // 0x06 is the low byte, 0x07 the high byte.
    let config = hal::pac::PWMX.config().read();
    let cyc_value = hal::pac::PWMX.reg_cycle().read().cyc_value();
    let duty_low = hal::pac::PWMX.pwm6_data().read().pwm6_data();
    let duty_high = hal::pac::PWMX.pwm7_data().read().pwm7_data();
    let out_enabled = hal::pac::PWMX.out_en().read().pwm5_out_en();
    let duty_readback = u16::from_le_bytes([duty_low, duty_high]);
    if config.cyc_mod() != 3
        || config.cycle_sel()
        || cyc_value != PERIOD - 1
        || duty_readback != DUTY
        || !out_enabled
    {
        error!(
            "register readback: cyc_mod {}, cycle_sel {}, cycle {}, duty {:#06x}, output {}",
            config.cyc_mod(),
            config.cycle_sel(),
            cyc_value,
            duty_readback,
            out_enabled
        );
        loop {
            core::hint::spin_loop();
        }
    }

    let probe = Input::new(p.PA14, Pull::Down);
    let mut timer = Timer::new(p.TMR2);
    timer.configure_counter(PeriodTicks::new(MAX_PERIOD).unwrap());
    timer.start();

    info!("16-bit PWM5 on PA13, jumper PA13 to PA14");

    let expected_period = u32::from(DIVISOR) * u32::from(PERIOD);
    let period = measure_period(&probe, &timer);
    let duty_pct = measure_duty(&probe, &timer, expected_period);
    let expected_duty = u64::from(DUTY) * 100 / u64::from(PERIOD);
    let freq = sysclk as u64 / expected_period as u64;

    let period_ok = period.abs_diff(expected_period) as u64 * 100 <= u64::from(expected_period);
    let duty_ok = (duty_pct as u64).abs_diff(expected_duty) <= 5;

    if period_ok && duty_ok {
        info!(
            "SELFTEST PASS: 16-bit period {} Fsys ticks (expected {}), {} Hz, duty {}% (expected {}%)",
            period, expected_period, freq, duty_pct, expected_duty
        );
    } else {
        error!(
            "SELFTEST FAIL: 16-bit period {} Fsys ticks (expected {}), duty {}% (expected {}%)",
            period, expected_period, duty_pct, expected_duty
        );
    }

    loop {
        core::hint::spin_loop();
    }
}

/// Median interval between `PERIOD_SAMPLES + 1` rising edges.
fn measure_period(probe: &Input<'_>, timer: &Timer<'_, hal::peripherals::TMR2>) -> u32 {
    let mut prev_high = probe.is_high();
    let mut last = 0u32;
    let mut have_last = false;
    let mut count = 0usize;
    let mut intervals = [0u32; PERIOD_SAMPLES];
    while count < PERIOD_SAMPLES {
        let high = probe.is_high();
        if high && !prev_high {
            let now = timer.count();
            if have_last {
                intervals[count] = now.wrapping_sub(last);
                count += 1;
            }
            last = now;
            have_last = true;
        }
        prev_high = high;
    }
    intervals.sort_unstable();
    intervals[PERIOD_SAMPLES / 2]
}

/// Sample the input over [`DUTY_PERIODS`] periods and return the high share.
fn measure_duty(probe: &Input<'_>, timer: &Timer<'_, hal::peripherals::TMR2>, period: u32) -> u8 {
    let start = timer.count();
    let mut high = 0u64;
    let mut samples = 0u64;
    while timer.count().wrapping_sub(start) < period * DUTY_PERIODS {
        if probe.is_high() {
            high += 1;
        }
        samples += 1;
    }
    (high * 100 / samples.max(1)) as u8
}
