//! Fill a buffer with timed automatic ADC conversions.
//!
//! Nothing is wired: the internal temperature sensor (channel 15) is converted
//! every 4096 system clocks, the longest `AUTO_CYCLE` period, which is 640 us
//! at the 6.4 MHz reset clock. The ADC-local DMA engine stores one 16-bit
//! sample per conversion, so the 32 samples cover about 20 ms.
//!
//! The spread across the buffer and the difference between its mean and a
//! single conversion are the checks. A temperature sensor is stable over that
//! window, so a wide spread means the engine stored something else.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::adc::{Adc, AutoCycle, Error};
use hal::dma::StaticSampleBuffer;
use panic_halt as _;
use qingke_rt::entry;

const LEN: usize = 32;

static SAMPLES: StaticSampleBuffer<LEN> = StaticSampleBuffer::new([0; LEN]);

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut adc = Adc::new_temperature(p.ADC);
    let buffer = SAMPLES.take().unwrap();

    defmt::info!("ADC auto sampling, {} samples", LEN);
    // The transfer borrows the converter, so it is resolved before the
    // comparison read below.
    let samples = match adc.start_auto(buffer, AutoCycle::LONGEST) {
        Ok(transfer) => match transfer.blocking_wait() {
            Ok(samples) => Some(samples),
            Err((error, _)) => {
                defmt::error!("auto transfer failed: {}", tag(error));
                None
            }
        },
        Err((error, _)) => {
            defmt::error!("auto transfer not started: {}", tag(error));
            None
        }
    };
    if let Some(samples) = samples {
        report(&mut adc, samples);
    }

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

fn report(adc: &mut Adc<'_, hal::peripherals::ADC>, samples: &[u16]) {
    let mut min = u16::MAX;
    let mut max = 0;
    let mut sum = 0u32;
    for &sample in samples {
        min = min.min(sample);
        max = max.max(sample);
        sum += u32::from(sample);
    }
    let mean = (sum / samples.len() as u32) as u16;
    let spread = max - min;

    match adc.read() {
        Ok(one_shot) => {
            let difference = mean.abs_diff(one_shot);
            defmt::info!(
                "counts {}..{}, mean {} = {} C; one-shot {} = {} C",
                min,
                max,
                mean,
                adc.temperature(mean),
                one_shot,
                adc.temperature(one_shot),
            );
            if spread <= 64 && difference <= 64 {
                defmt::info!("AUTO PASS");
            } else {
                defmt::error!("AUTO FAIL: spread {}, difference {}", spread, difference);
            }
        }
        Err(error) => defmt::error!("one-shot read failed: {}", tag(error)),
    }
}

fn tag(error: Error) -> &'static str {
    match error {
        Error::Timeout => "timeout",
        Error::Buffer => "buffer",
        _ => "error",
    }
}
