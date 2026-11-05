//! Scan the I2C0 bus on the CH585 board and log every address that answers.
//!
//! SCL is PB13 and SDA is PB12, the default pair. Both lines need an external
//! pull-up to 3.3 V; the internal pull-up is only a bias while the controller is
//! idle. The bus runs at 100 kHz, which needs a system clock between 2 MHz and
//! 36 MHz, so the board boots at the 24 MHz PLL rather than the 5.333 MHz reset
//! clock.
//!
//! Each address is probed with an empty write. A device acknowledges its address
//! and the probe logs it; a missing device returns a NACK and is skipped. A bus
//! error or a timeout stops the scan, since the bus is then no longer idle.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::i2c::{Config, I2c};
use hal::rcc::Config as ClockConfig;
use hal::time::Hertz;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: ClockConfig::HSI_PLL_24MHZ,
    });

    let mut bus = I2c::new_blocking::<0>(p.I2C0, p.PB13, p.PB12, Config::new(Hertz(100_000))).unwrap();

    defmt::info!("I2C0 scan on PB13/PB12 at 100 kHz");
    for address in 0x08..=0x77 {
        match bus.blocking_write(address, &[]) {
            Ok(()) => defmt::info!("0x{:02x} ACKed", address),
            Err(hal::i2c::Error::NoAcknowledge(_)) => {}
            Err(hal::i2c::Error::Bus) => {
                defmt::error!("0x{:02x}: bus error", address);
                break;
            }
            Err(hal::i2c::Error::ArbitrationLoss) => {
                defmt::error!("0x{:02x}: arbitration lost", address);
                break;
            }
            Err(hal::i2c::Error::Overrun) => {
                defmt::error!("0x{:02x}: overrun", address);
                break;
            }
            Err(hal::i2c::Error::Timeout) => {
                defmt::error!("0x{:02x}: timed out", address);
                break;
            }
            Err(hal::i2c::Error::Address | hal::i2c::Error::ZeroLength) => {
                defmt::error!("0x{:02x}: rejected by the driver", address);
                break;
            }
            Err(_) => {
                defmt::error!("0x{:02x}: failed", address);
                break;
            }
        }
    }
    defmt::info!("scan done");

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
