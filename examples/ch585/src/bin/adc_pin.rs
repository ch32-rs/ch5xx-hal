//! Read one ADC channel on PA4 and log the raw count.
//!
//! PA4 is channel 0. Leave it open and the count floats; tie it to GND for a
//! reading near zero or to 3.3 V for a reading near full scale. The driver
//! disconnects the pad's digital input, so the pin does not need a resistor.
//!
//! The count is corrected by the converter's own offset, measured once at start
//! with the input shorted inside the block.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::adc::{Adc, Config};
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut adc = Adc::new_pin(p.ADC, p.PA4, Config::default());
    let offset = adc.calibrate().unwrap();
    defmt::info!("PA4 offset {}", offset);

    for _ in 0..4 {
        match adc.read() {
            Ok(sample) => defmt::info!("PA4: {} counts", sample as i16 + offset),
            Err(hal::adc::Error::Timeout) => defmt::error!("timed out"),
            Err(_) => defmt::error!("failed"),
        }
    }

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
