//! Scan the CH592X I2C0 bus and log every address that answers.
//!
//! SCL is PB15 and SDA is PB14 on this die (CH592 datasheet table 7-7; the
//! CH585 pair is PB13/PB12). Both lines need an external pull-up to 3.3 V; the
//! internal pull-up is only a bias while the controller is idle. The EVT board
//! has no I2C device, so a scan with nothing wired checks the driver's idle and
//! timeout path rather than a device.
//!
//! Each address is probed with an empty write. A device acknowledges its address
//! and the probe logs it; a missing device returns a NACK and is skipped. A bus
//! error or a timeout stops the scan, since the bus is then no longer idle.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::i2c::{Config, I2c};
use hal::time::Hertz;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut bus = match I2c::new_blocking::<0>(p.I2C0, p.PB15, p.PB14, Config::new(Hertz(100_000))) {
        Ok(bus) => bus,
        Err(error) => {
            defmt::error!("I2C init error {}", error as u8);
            loop {
                core::hint::spin_loop();
            }
        }
    };
    let mut acked = 0;

    defmt::info!("I2C0 scan on PB15/PB14 at 100 kHz");
    for address in 0x08..=0x77 {
        match bus.blocking_write(address, &[]) {
            Ok(()) => {
                defmt::info!("0x{:02x} ACKed", address);
                acked += 1;
            }
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
    defmt::info!("scan done, {} ACK(s)", acked);

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
