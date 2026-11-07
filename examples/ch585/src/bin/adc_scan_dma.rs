//! Scan three ADC channels with the ADC-local DMA engine.
//!
//! PA4, PA5 and PA6 are channels 0, 1 and 10; the tuple order is the scan order.
//! Tie PA4 to GND and PA5 to 3.3 V for two known readings. PA6 has no external
//! connection here and its count will wander.
//!
//! Each buffer holds ten passes of the three channels, so the transfer also
//! exercises the DMA length rather than a single pass. Completion is polled:
//! the ADC interrupt is asserted by end-of-conversion and cannot be masked off
//! from the scan end flags, so there is no interrupt-driven wait.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::adc::{Config, Error, Scan};
use hal::delay::Delay;
use hal::dma::StaticSampleBuffer;
use panic_halt as _;
use qingke_rt::entry;

/// Ten passes over three channels.
const LEN: usize = 30;
const CHANNELS: usize = 3;

static SAMPLES: StaticSampleBuffer<LEN> = StaticSampleBuffer::new([0; LEN]);

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut scan = Scan::new(p.ADC, (p.PA4, p.PA5, p.PA6), Config::default());
    let mut delay = Delay;
    let mut buffer: &'static mut [u16] = SAMPLES.take().unwrap();
    defmt::info!("ADC scan: PA4, PA5, PA6");

    loop {
        match scan.read(buffer) {
            Ok(transfer) => match transfer.blocking_wait() {
                Ok(samples) => {
                    log_passes(samples);
                    buffer = samples;
                }
                Err((error, samples)) => {
                    defmt::error!("scan failed: {}", tag(error));
                    buffer = samples;
                }
            },
            Err((error, samples)) => {
                defmt::error!("scan not started: {}", tag(error));
                buffer = samples;
            }
        }
        delay.delay_ms(500);
    }
}

fn log_passes(samples: &[u16]) {
    for (index, pass) in samples.chunks(CHANNELS).enumerate() {
        defmt::info!("pass {}: {} {} {}", index, pass[0], pass[1], pass[2]);
    }
}

fn tag(error: Error) -> &'static str {
    match error {
        Error::Timeout => "timeout",
        Error::Buffer => "buffer",
        _ => "error",
    }
}
