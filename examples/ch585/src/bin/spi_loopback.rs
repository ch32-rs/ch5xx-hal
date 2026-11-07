//! Loop SPI0 back to itself and check the bytes that come back.
//!
//! Wire PA14 (MOSI) to PA15 (MISO). SCK is PA13 and is left unconnected. The
//! controller has no chip select, so the example drives PA12 around each transfer
//! the way the vendor examples do.
//!
//! The bus runs at 1 MHz from the 24 MHz PLL. The first transfer is mode 0 and
//! the second is mode 3, over a buffer longer than the eight-byte FIFO. Each one
//! logs whether the bytes that come back match what was sent.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::gpio::Level;
use hal::rcc::Config as ClockConfig;
use hal::spi::{chip_select, Config, Spi};
use hal::time::Hertz;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: ClockConfig::HSI_PLL_24MHZ,
    });

    let mut bus = Spi::new_blocking::<0>(p.SPI0, p.PA13, p.PA14, p.PA15, Config::new(Hertz(1_000_000))).unwrap();
    let mut cs = chip_select(p.PA12, Level::High);

    defmt::info!("SPI0 loopback, wire PA14 to PA15");

    check(
        "mode 0",
        &mut bus,
        &mut cs,
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
    check("mode 3", &mut bus, &mut cs, &long);

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// Send `bytes` and log whether the same bytes come back.
fn check<T: hal::spi::Instance>(
    mode: &str,
    bus: &mut Spi<'_, T, hal::mode::Blocking>,
    cs: &mut hal::spi::ChipSelect<'_>,
    bytes: &[u8],
) {
    let mut received = [0u8; 40];
    let received = &mut received[..bytes.len()];
    match cs.select(|| bus.blocking_transfer(received, bytes)) {
        Ok(()) if received == bytes => defmt::info!("{}: {} bytes matched", mode, bytes.len()),
        Ok(()) => defmt::error!("{}: {} bytes mismatched", mode, bytes.len()),
        Err(hal::spi::Error::Timeout) => defmt::error!("{}: {} bytes timed out", mode, bytes.len()),
        Err(hal::spi::Error::Overrun) => defmt::error!("{}: {} bytes overran the fifo", mode, bytes.len()),
        Err(_) => defmt::error!("{}: {} bytes failed", mode, bytes.len()),
    }
}
