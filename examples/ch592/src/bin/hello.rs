//! Boot at the default clock and log over defmt/RTT.
//!
//! ```console
//! PROBE_RS_PROBE=1a86:8010:F93D8F067E10 cargo run --release --bin hello
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::{error, info};
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

/// Reset default on CH592: 32 MHz crystal / 5.
const EXPECTED_FSYS: u32 = 6_400_000;

#[entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    let mut delay = hal::delay::Delay;
    let mut count = 0u32;

    let sysclk = hal::rcc::clocks().sysclk.0;
    info!("CH592X up, Fsys {} Hz", sysclk);
    if sysclk == EXPECTED_FSYS {
        info!("clock {}, SELFTEST PASS", sysclk);
    } else {
        error!("clock {}, expected {}, SELFTEST FAIL", sysclk, EXPECTED_FSYS);
    }

    loop {
        info!("tick {} at {} Hz", count, hal::rcc::clocks().sysclk.0);
        count = count.wrapping_add(1);
        delay.delay_ms(500);
    }
}
