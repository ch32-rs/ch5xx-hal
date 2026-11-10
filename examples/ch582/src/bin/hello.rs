//! Boot at the reset clock and log over defmt/RTT.
//!
//! ```console
//! PROBE_RS_PROBE=1a86:8010:2FC48F06B4A8 cargo run --release --bin hello
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::gpio::{Flex, Level, Pull, Speed};
use defmt::{error, info};
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

/// Reset default on CH58x: 32 MHz crystal / 5.
const EXPECTED_FSYS: u32 = 6_400_000;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = hal::delay::Delay;

    let nominal = hal::rcc::clocks().sysclk.0;
    let measured = hal::delay::measured_sysclk().0;
    info!("CH582 up, Fsys {} Hz nominal, {} Hz measured", nominal, measured);

    // Drive a pad and read it back through the input buffer. PA4 is not one of
    // the debug pads (PA13/PA14), so the test can run under the same debugger.
    let mut pin = Flex::new(p.PA4);
    pin.set_as_output(Speed::Low);
    pin.set_level(Level::High);
    let high = pin.is_high();
    pin.set_level(Level::Low);
    let low = pin.is_low();
    pin.set_as_input(Pull::None);
    if high && low {
        info!("GPIO PA4 readback PASS");
    } else {
        error!("GPIO PA4 readback FAIL: high={} low={}", high, low);
    }

    if nominal == EXPECTED_FSYS {
        info!("clock {}, SELFTEST PASS", nominal);
    } else {
        error!("clock {}, expected {}, SELFTEST FAIL", nominal, EXPECTED_FSYS);
    }

    let mut count = 0u32;
    loop {
        info!("tick {} at {} Hz", count, hal::rcc::clocks().sysclk.0);
        count = count.wrapping_add(1);
        delay.delay_ms(500);
    }
}
