//! Breathe an external LED on PWM4 (PA12).
//!
//! The on-board PB16/PB17 LEDs are plain GPIO pins with no PWM alternate
//! function, so this example needs an external LED. Wire PA12 through a
//! resistor to the LED anode and the cathode to GND; the channel is active high.
//!
//! The block runs from the 24 MHz PLL with divisor 235 and a 256-clock cycle, so
//! the pin switches at about 400 Hz and the eye sees the average brightness.
//! Duty ramps from 0 to 255 and back, five counts every 20 ms.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::delay::Delay;
use hal::pwm::{Config, Cycle, Polarity, Pwm};
use hal::rcc::Config as ClockConfig;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: ClockConfig::HSI_PLL_24MHZ,
    });

    let mut pwm = Pwm::new(p.PWMX, Config::new(235, Cycle::Clocks256)).unwrap();
    let mut channel = pwm.channel(p.PA12, Polarity::ActiveHigh);
    let mut delay = Delay;

    defmt::info!("PWM4 on PA12, about 400 Hz; ramp the duty");
    loop {
        let mut duty = 0;
        while duty < 255 {
            channel.set_duty(duty);
            delay.delay_ms(20);
            duty += 5;
        }
        channel.set_duty(255);
        delay.delay_ms(20);
        while duty > 0 {
            duty -= 5;
            channel.set_duty(duty);
            delay.delay_ms(20);
        }
    }
}
