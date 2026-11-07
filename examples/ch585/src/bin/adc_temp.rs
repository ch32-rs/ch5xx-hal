//! Read the CH585's internal temperature sensor.
//!
//! Nothing is wired. The converter measures its own sensor and the reading is
//! turned into degrees Celsius with the calibration value stored in the info
//! flash. A room-temperature result is the check; the raw count is logged too.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::adc::Adc;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut adc = Adc::new_temperature(p.ADC);

    defmt::info!("ADC temperature");
    for _ in 0..4 {
        match adc.read() {
            Ok(sample) => {
                let degrees = adc.temperature(sample);
                defmt::info!("{} counts, {} C", sample, degrees);
            }
            Err(hal::adc::Error::Timeout) => defmt::error!("timed out"),
            Err(_) => defmt::error!("failed"),
        }
    }

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
