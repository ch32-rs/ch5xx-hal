//! Step PWM4 through five duty cycles on PA12.
//!
//! The block runs from the 24 MHz PLL with divisor 235 and a 256-clock cycle, so
//! the pin toggles at about 400 Hz. Each step holds for a second and is logged.
//! PA12 is not an on-board LED; probe it to see the duty change. A duty of 255 is
//! the highest the register can express on a 256-clock cycle.

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

    defmt::info!("PWM4 on PA12, about 400 Hz");
    loop {
        for duty in [0u16, 64, 128, 192, 255] {
            channel.set_duty(duty);
            defmt::info!("duty {}", duty);
            delay.delay_ms(1000);
        }
    }
}
