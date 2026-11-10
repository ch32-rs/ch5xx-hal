//! Scan all twelve CH592 ADC channels with the ADC-local DMA engine.
//!
//! The tuple order is the scan order. CH592 routes AIN0-AIN5 to
//! PA4/PA5/PA12/PA13/PA14/PA15, AIN8/AIN9 to PB0/PB6 and AIN10-AIN13 to
//! PA6/PA7/PA8/PA9; AIN6 and AIN7 do not exist. This is the largest sequence
//! the block accepts, so it exercises the three `SCAN_CFG2` entries and the
//! CH592 `SCAN_NUM` bit position.
//!
//! Nothing has to be wired. Tie a sampled pin to GND or 3.3 V for a known
//! column; the others are floating inputs and their counts will wander.
//!
//! Each buffer holds three passes, so the transfer also exercises the DMA
//! length rather than a single pass. Completion is polled: the ADC interrupt is
//! asserted by end-of-conversion and cannot be masked off from the scan end
//! flags, so there is no interrupt-driven wait.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::adc::{Config, Error, Scan};
use hal::delay::Delay;
use hal::dma::StaticSampleBuffer;
use panic_halt as _;
use qingke_rt::entry;

/// Three passes over twelve channels.
const CHANNELS: usize = 12;
const LEN: usize = 3 * CHANNELS;

static SAMPLES: StaticSampleBuffer<LEN> = StaticSampleBuffer::new([0; LEN]);

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut scan = Scan::new(
        p.ADC,
        (
            p.PA4, p.PA5, p.PA12, p.PA13, p.PA14, p.PA15, p.PB0, p.PB6, p.PA6, p.PA7, p.PA8, p.PA9,
        ),
        Config::default(),
    );
    let mut delay = Delay;
    let mut buffer: &'static mut [u16] = SAMPLES.take().unwrap();
    defmt::info!("ADC scan: AIN0-5, AIN8-13");

    loop {
        match scan.read(buffer) {
            Ok(transfer) => match transfer.blocking_wait() {
                Ok(samples) => {
                    log_passes(samples);
                    defmt::info!("SCAN PASS");
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
        defmt::info!(
            "pass {}: {} {} {} {} {} {} {} {} {} {} {} {}",
            index,
            pass[0],
            pass[1],
            pass[2],
            pass[3],
            pass[4],
            pass[5],
            pass[6],
            pass[7],
            pass[8],
            pass[9],
            pass[10],
            pass[11],
        );
    }
}

fn tag(error: Error) -> &'static str {
    match error {
        Error::Timeout => "timeout",
        Error::Buffer => "buffer",
        _ => "error",
    }
}
