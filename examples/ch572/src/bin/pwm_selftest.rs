//! CH572 PWM register selftest; no wiring is required.
//!
//! PA7 (PWM1), PA2 (PWM2), PA3 (PWM3), PA4 (PWM4) and PA8 (PWM5) are driven
//! briefly; leave them unconnected. Debug and reset pins are not used. This
//! checks register readback against the CH572 EVT `PWMX_8Bit` and `PWMX_16Bit`
//! examples, not the output waveform, frequency or actual duty cycle.
//!
//! PA2/PA3 are also UART0 and PA8 is also I2C0 SCL, so do not enable those
//! drivers in the same program.

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
    let mut passed = 0;

    // Fixed 8-bit cycles, matching `PWMX_CycleCfg` in the EVT.
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
    for (cycle, mode, short, max) in cases {
        let pwm = Pwm::new(p.PWMX.reborrow(), Config::new(7, cycle)).unwrap();
        let cfg = regs.config().read();
        let mut ok = cfg.cyc_mod() == mode && cfg.cycle_sel() == short && regs.clock_div().read().clock_div() == 7;

        let mut ch = pwm.channel(p.PA7.reborrow(), Polarity::ActiveHigh);
        ok &= regs.out_en().read().pwm1_out_en();
        ok &= !regs.polar().read().pwm1_polar();
        ok &= ch.max_duty_cycle() == max;
        for duty in [0, max / 2, max, max + 1] {
            ch.set_duty(duty);
            ok &= regs.pwm1_3_data().read().pwm1_data__pwm1_data() as u16 == duty.min(max);
        }
        drop(ch);
        ok &= !regs.out_en().read().pwm1_out_en();

        if ok {
            passed += 1;
            info!("8-bit {} clocks: PASS", cycle.clocks());
        } else {
            error!(
                "8-bit {} clocks: FAIL (mode {}, short {}, divider {})",
                cycle.clocks(),
                cfg.cyc_mod(),
                cfg.cycle_sel(),
                regs.clock_div().read().clock_div()
            );
        }
    }

    // 16-bit mode: one halfword register per channel, one cycle register for
    // PWM1-3 and one for PWM4/5.
    {
        let pwm = Pwm::new(p.PWMX.reborrow(), Config::new(4, Cycle::Clocks16(60000))).unwrap();
        let cfg = regs.config().read();
        let mut ok = cfg.cyc_mod() == 3
            && regs.cyc_value().read().cyc_value() == 59999
            && regs.cyc1_value().read().cyc1_value() == 59999;

        let mut ch1 = pwm.channel(p.PA7.reborrow(), Polarity::ActiveLow);
        let mut ch2 = pwm.channel(p.PA2.reborrow(), Polarity::ActiveHigh);
        let mut ch3 = pwm.channel(p.PA3.reborrow(), Polarity::ActiveHigh);
        let mut ch4 = pwm.channel(p.PA4.reborrow(), Polarity::ActiveHigh);
        let mut ch5 = pwm.channel(p.PA8.reborrow(), Polarity::ActiveHigh);

        ok &= regs.polar().read().pwm1_polar()
            && !regs.polar().read().pwm2_polar()
            && !regs.polar().read().pwm3_polar()
            && !regs.polar().read().pwm4_polar()
            && !regs.polar().read().pwm5_polar();
        ok &= regs.out_en().read().pwm1_out_en()
            && regs.out_en().read().pwm2_out_en()
            && regs.out_en().read().pwm3_out_en()
            && regs.out_en().read().pwm4_out_en()
            && regs.out_en().read().pwm5_out_en();
        ok &= ch1.max_duty_cycle() == 60000
            && ch2.max_duty_cycle() == 60000
            && ch3.max_duty_cycle() == 60000
            && ch4.max_duty_cycle() == 60000
            && ch5.max_duty_cycle() == 60000;

        ch1.set_duty(30000);
        ch2.set_duty(15000);
        ch3.set_duty(45000);
        ch4.set_duty(30000);
        ch5.set_duty(15000);
        ok &= regs.pwm1_data().read().pwm1_data() == 30000
            && regs.pwm2_data().read().pwm2_data() == 15000
            && regs.pwm3_data().read().pwm3_data() == 45000
            && regs.pwm4_data().read().pwm4_data() == 30000
            && regs.pwm5_data().read().pwm5_data() == 15000;

        pwm.set_stagger(StaggerPair::Pwm4_5, true);
        ok &= regs.config().read().pwm4_5_stag_en();
        let _phase = pwm.stagger_phase();
        pwm.set_stagger(StaggerPair::Pwm4_5, false);
        ok &= !regs.config().read().pwm4_5_stag_en();

        drop(ch1);
        drop(ch2);
        drop(ch3);
        drop(ch4);
        drop(ch5);
        drop(pwm);
        ok &= !regs.out_en().read().pwm1_out_en()
            && !regs.out_en().read().pwm2_out_en()
            && !regs.out_en().read().pwm3_out_en()
            && !regs.out_en().read().pwm4_out_en()
            && !regs.out_en().read().pwm5_out_en();

        if ok {
            passed += 1;
            info!("16-bit 60000 clocks: PASS");
        } else {
            error!("16-bit 60000 clocks: FAIL");
        }
    }

    if passed == 9 {
        info!("SELFTEST PASS: 9/9 PWM register cases (not waveform tested)");
    } else {
        error!("SELFTEST FAIL: {}/9 PWM register cases", passed);
    }
    loop {
        core::hint::spin_loop();
    }
}
