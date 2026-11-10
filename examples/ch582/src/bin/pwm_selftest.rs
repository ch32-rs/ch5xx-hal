//! CH582 PWM register selftest; no wiring is required.
//!
//! PA12 (PWM4) and PB0 (PWM6) are driven briefly; leave them unconnected.
//! Debug and reset pins are not used. This checks register readback, not the
//! output waveform, frequency, polarity on the pin, or actual duty cycle.
//! WCH CH583DS1 section 11.2 describes the CH582 PWM register encodings.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::{error, info};
use defmt_rtt as _;
use embedded_hal::pwm::SetDutyCycle;
use hal::pwm::{Config, Cycle, Polarity, Pwm, StaggerPair};
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let mut p = hal::init(hal::rcc::Config::default().into());
    let regs = hal::pac::PWMX;
    let cases = [
        (Cycle::Clocks256, 0, false, 255),
        (Cycle::Clocks255, 0, true, 255),
        (Cycle::Clocks128, 1, false, 128),
        (Cycle::Clocks127, 1, true, 127),
        (Cycle::Clocks64, 2, false, 64),
        (Cycle::Clocks63, 2, true, 63),
        (Cycle::Clocks32, 3, false, 32),
        (Cycle::Clocks31, 3, true, 31),
    ];
    let mut passed = 0;
    // Establish a known stagger state before testing preservation across new().
    {
        let pwm = Pwm::new(p.PWMX.reborrow(), Config::new(7, Cycle::Clocks256)).unwrap();
        pwm.set_stagger(StaggerPair::Pwm4_5, true);
        pwm.set_stagger(StaggerPair::Pwm6_7, false);
        pwm.set_stagger(StaggerPair::Pwm8_9, true);
        pwm.set_stagger(StaggerPair::Pwm10_11, false);
    }

    for (cycle, mode, short, max) in cases {
        let pwm = Pwm::new(p.PWMX.reborrow(), Config::new(7, cycle)).unwrap();
        let cfg = regs.config().read();
        let mut ok = cfg.cyc_mod() == mode
            && cfg.cycle_sel() == short
            && cfg.pwm4_5_stag_en()
            && !cfg.pwm6_7_stag_en()
            && cfg.pwm8_9_stag_en()
            && !cfg.pwm10_11_stag_en()
            && regs.clock_div().read().clock_div() == 7;
        {
            let mut a = pwm.channel(p.PA12.reborrow(), Polarity::ActiveHigh);
            let mut b = pwm.channel(p.PB0.reborrow(), Polarity::ActiveLow);
            ok &= regs.out_en().read().pwm4_out_en() && regs.out_en().read().pwm6_out_en();
            ok &= !regs.polar().read().pwm4_polar() && regs.polar().read().pwm6_polar();
            ok &= a.max_duty_cycle() == max && b.max_duty_cycle() == max;
            for duty in [0, max / 2, max, max + 1] {
                a.set_duty(duty);
                b.set_duty(max - duty.min(max));
                ok &= regs.pwm4_data().read().pwm4_data() as u16 == duty.min(max);
                ok &= regs.pwm6_data().read().pwm6_data() as u16 == max - duty.min(max);
            }
        }
        ok &= !regs.out_en().read().pwm4_out_en() && !regs.out_en().read().pwm6_out_en();
        if ok {
            passed += 1;
            info!("PWM {} clocks: PASS", cycle.clocks());
        } else {
            error!(
                "PWM {} clocks: FAIL (mode {}, short {}, divider {})",
                cycle.clocks(),
                cfg.cyc_mod(),
                cfg.cycle_sel(),
                regs.clock_div().read().clock_div()
            );
        }
    }

    if passed == 8 {
        info!("SELFTEST PASS: 8/8 PWM register cases (not waveform tested)");
    } else {
        error!("SELFTEST FAIL: {}/8 PWM register cases", passed);
    }
    loop {
        core::hint::spin_loop();
    }
}
