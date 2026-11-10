//! Loop SPI0 back to itself on CH592X and check the bytes that come back.
//!
//! The EVT board's P2 header carries PA13 (SCK), PA14 (MOSI) and PA15 (MISO);
//! PA12, the pin the CH585 example uses as a chip select, is not broken out, so
//! this example leaves chip select out. Only MOSI has to be wired to MISO: pin
//! 14 (PA14) to pin 16 (PA15) on P2.
//!
//! The bus runs from the 6.4 MHz default clock. The first transfer is mode 0 and
//! the second is mode 3, over a buffer longer than the eight-byte FIFO.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::rcc::Config as ClockConfig;
use hal::spi::{Config, Spi};
use hal::time::Hertz;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: ClockConfig::default(),
    });

    let mut bus = match Spi::new_blocking::<0>(p.SPI0, p.PA13, p.PA14, p.PA15, Config::new(Hertz(1_000_000))) {
        Ok(bus) => bus,
        Err(error) => {
            defmt::error!("SPI init error {}", error as u8);
            loop {
                core::hint::spin_loop();
            }
        }
    };

    defmt::info!("SPI0 loopback, wire P2 pin 14 (PA14) to pin 16 (PA15)");

    check(
        "mode 0",
        &mut bus,
        &[0x00, 0xFF, 0xA5, 0x5A, 0x01, 0x80, 0x0F, 0xF0, 0x3C, 0xC3],
    );

    // Mode 3 idles SCK high. The same wire has to pass both clock modes.
    let mut config = Config::new(Hertz(1_000_000));
    config.mode = embedded_hal::spi::MODE_3;
    bus.set_config(config).unwrap();

    let mut long = [0u8; 40];
    for (index, byte) in long.iter_mut().enumerate() {
        *byte = index as u8 * 7 + 3;
    }
    check("mode 3", &mut bus, &long);

    defmt::info!("SPI loopback checks done");

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// Send `bytes` and log whether the same bytes come back.
fn check<T: hal::spi::Instance>(mode: &str, bus: &mut Spi<'_, T, hal::mode::Blocking>, bytes: &[u8]) {
    let mut received = [0u8; 40];
    let received = &mut received[..bytes.len()];
    match bus.blocking_transfer(received, bytes) {
        Ok(()) if received == bytes => defmt::info!("{}: {} bytes matched", mode, bytes.len()),
        // On an unwired MISO pad the pull-up reads `0xFF`; log the first bytes
        // so the log distinguishes that from a bus that never clocked.
        Ok(()) => defmt::error!(
            "{}: {} bytes mismatched, got {=[u8]:02x}",
            mode,
            bytes.len(),
            &received[..4]
        ),
        Err(hal::spi::Error::Timeout) => defmt::error!("{}: {} bytes timed out", mode, bytes.len()),
        Err(hal::spi::Error::Overrun) => defmt::error!("{}: {} bytes overran the fifo", mode, bytes.len()),
        Err(_) => defmt::error!("{}: {} bytes failed", mode, bytes.len()),
    }
}
