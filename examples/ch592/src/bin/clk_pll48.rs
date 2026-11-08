//! Boot at 48 MHz from the CH592's 480 MHz PLL and report the clock over RTT.
//!
//! This exercises the PLL branch of `rcc`: the divider, the Flash timing byte
//! and the read-back checks. 48 MHz is also the crystal-independent USB rate.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::rcc;
use defmt::{error, info};
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let config = hal::Config::from(rcc::Config::PLL_48MHZ);
    let _p = hal::init(config);

    let mut delay = hal::delay::Delay;

    let sysclk = rcc::clocks().sysclk.0;
    info!("CH592X PLL up, Fsys {} Hz", sysclk);
    if sysclk == 48_000_000 {
        info!("clock {}, SELFTEST PASS", sysclk);
    } else {
        error!("clock {}, expected 48000000, SELFTEST FAIL", sysclk);
    }

    loop {
        info!("alive at {} Hz", rcc::clocks().sysclk.0);
        delay.delay_ms(500);
    }
}
