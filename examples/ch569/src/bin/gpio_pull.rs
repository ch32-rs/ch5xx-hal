//! GPIO pull, drive and readback on the CH569's split registers.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::gpio::{Flex, Level, Pull, Speed};
use ch5xx_hal::pac;
use defmt::info;
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

fn report(tag: &str, pin: u32) {
    let gpio = pac::GPIO;
    info!(
        "{}: PIN={=u32:#010x} DIR={=u32:#010x} PU={=u32:#010x} PD={=u32:#010x} DRV={=u32:#010x}",
        tag,
        gpio.pb_pin().read().0 & pin,
        gpio.pb_dir().read().0 & pin,
        gpio.pb_pu().read().0 & pin,
        gpio.pb_pd().read().0 & pin,
        gpio.pb_drv().read().0 & pin,
    );
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    // PB22 drives LED1 (active low) through a 1 kOhm resistor to 3V3, so it has
    // a defined level in both directions.
    const PIN: u32 = 1 << 22;
    let mut flex = Flex::new(p.PB22);

    flex.set_as_input(Pull::Up);
    report("input pull-up", PIN);
    flex.set_as_input(Pull::Down);
    report("input pull-down", PIN);
    flex.set_as_input(Pull::None);
    report("input float", PIN);

    flex.set_as_output(Speed::Low);
    flex.set_level(Level::High);
    report("output high, 8 mA", PIN);
    flex.set_level(Level::Low);
    report("output low, 8 mA", PIN);
    flex.set_as_output(Speed::High);
    report("output low, 16 mA", PIN);

    info!("gpio split-register walk done");

    loop {
        core::hint::spin_loop();
    }
}
