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
//! The period is timed with TMR2 on `Fsys`, not the RTC: the 32 kHz domain
//! defaults to the internal RC (`CK32K_CONFIG.RB_CLK_OSC32K_XT` is clear) and
//! the counter divides by 32768, which does not track the 32.000 kHz nominal
//! closely enough for a cycle-accurate period. The duty is measured over forty
//! periods, so the sample count is high enough for a percentage.
//!
//! After the plain check, PWM4/PWM5 stagger is enabled with only PWM5 driving.
//! The pair then takes turns, so PWM5 appears on every other cycle and its
//! measured period doubles; disabling it restores the original period.
//!
//! Both period checks take the median of several edge intervals rather than a
//! span average. A defmt write to the RTT buffer can stall the core for
//! milliseconds, and an edge that falls inside the stall is never sampled; the
//! lost edge then inflates one interval by whole periods. The median ignores
//! those outliers, and the status sampling shows both stagger phases occur.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::{error, info};
use defmt_rtt as _;
use hal::gpio::{Input, Pull};
use hal::pwm::{Config, Cycle, Polarity, Pwm, StaggerPair};
use hal::timer::low_level::Timer;
use hal::timer::{PeriodTicks, MAX_PERIOD};
use panic_halt as _;
use qingke_rt::entry;

/// PWM clock divisor.
const DIVISOR: u8 = 64;
/// Compare value for the 256-clock cycle, about half duty.
const DUTY: u16 = 0x80;
/// Edge intervals collected for the period check. Odd, so the median is a
/// measured interval.
const PERIOD_SAMPLES: usize = 15;
/// Periods sampled for the duty check.
const DUTY_PERIODS: u32 = 40;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let sysclk = hal::rcc::clocks().sysclk.0;

    let pwm = match Pwm::new(p.PWMX, Config::new(DIVISOR, Cycle::Clocks256)) {
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

    let expected_period = u32::from(DIVISOR) * u32::from(Cycle::Clocks256.clocks());
    let period = measure_period(&probe, &timer);
    let duty_pct = measure_duty(&probe, &timer, expected_period);
    info!("plain: period {} Fsys ticks (expected {}), duty {}%", period, expected_period, duty_pct);

    // With the pair staggered and only PWM5 driving, the odd channel owns
    // every other cycle, so the period seen on PA13 doubles.
    pwm.set_stagger(StaggerPair::Pwm4_5, true);
    let staggered = measure_period(&probe, &timer);
    let (phase_even, phase_odd) = sample_phase(&pwm, &timer, expected_period * 8);
    pwm.set_stagger(StaggerPair::Pwm4_5, false);
    let restored = measure_period(&probe, &timer);
    info!(
        "stagger: period {} (expected {}), status even {} odd {}, restored {}",
        staggered,
        period * 2,
        phase_even,
        phase_odd,
        restored
    );

    let expected_duty = u64::from(DUTY) * 100 / 255;
    let within_one_percent = |measured: u32, expected: u32| {
        measured.abs_diff(expected) as u64 * 100 <= u64::from(expected)
    };
    let freq = sysclk as u64 / expected_period as u64;

    if within_one_percent(period, expected_period)
        && duty_pct.abs_diff(expected_duty as u8) <= 5
        && within_one_percent(staggered, period * 2)
        && phase_even > 0
        && phase_odd > 0
        && within_one_percent(restored, period)
    {
        info!(
            "SELFTEST PASS: period {} Fsys ticks (expected {}), {} Hz, duty {}% (expected {}%)",
            period, expected_period, freq, duty_pct, expected_duty
        );
    } else {
        error!(
            "SELFTEST FAIL: period {} (expected {}), duty {}%, staggered {} (expected {}), phase {}:{}, restored {}",
            period, expected_period, duty_pct, staggered, period * 2, phase_even, phase_odd, restored
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

/// Count reads of [`Pwm::stagger_phase`] for `span` `Fsys` ticks.
fn sample_phase(
    pwm: &Pwm<'_, hal::peripherals::PWMX>,
    timer: &Timer<'_, hal::peripherals::TMR2>,
    span: u32,
) -> (u32, u32) {
    let start = timer.count();
    let mut even = 0u32;
    let mut odd = 0u32;
    while timer.count().wrapping_sub(start) < span {
        if pwm.stagger_phase() {
            odd += 1;
        } else {
            even += 1;
        }
    }
    (even, odd)
}
