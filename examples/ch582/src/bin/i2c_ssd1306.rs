//! Draw a smiley on a 128x64 SSD1306 I2C display.
//!
//! Connect PB13 to SCL, PB12 to SDA, and share ground. The bus needs external
//! pull-ups to a CH582-safe supply; many OLED modules include them. Use a
//! compatible supply for the display. Only 0x3c and 0x3d are probed, with
//! address-only writes; an ACK does not identify the controller.
//!
//! This configuration is for a 128x64 SSD1306 with an internal charge pump;
//! other geometries, controllers, and external-VCC modules may need changes.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::i2c::{Config, Error, I2c};
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = hal::delay::Delay;
    delay.delay_ms(100);

    let mut bus = match I2c::new_blocking::<0>(p.I2C0, p.PB13, p.PB12, Config::default()) {
        Ok(bus) => bus,
        Err(error) => {
            defmt::error!("I2C configuration failed: {:?}", defmt::Debug2Format(&error));
            halt();
        }
    };
    defmt::info!("SSD1306: probing 0x3c and 0x3d at 100 kHz (PB13/PB12)");

    let address = match find_display(&mut bus) {
        Ok(Some(address)) => address,
        Ok(None) => {
            defmt::error!("No ACK at 0x3c or 0x3d");
            halt();
        }
        Err(error) => {
            defmt::error!("I2C probe failed: {:?}", defmt::Debug2Format(&error));
            halt();
        }
    };
    defmt::info!("Address 0x{=u8:02x} ACKed; initializing 128x64 SSD1306", address);

    match draw(&mut bus, address) {
        Ok(()) => defmt::info!("SSD1306 initialization and smiley writes complete (display not read back)"),
        Err(error) => defmt::error!(
            "SSD1306 initialization or drawing failed: {:?}",
            defmt::Debug2Format(&error)
        ),
    }
    halt()
}

fn find_display<T: hal::i2c::Instance>(bus: &mut I2c<'_, T, hal::mode::Blocking>) -> Result<Option<u8>, Error> {
    for address in [0x3c, 0x3d] {
        match bus.blocking_write(address, &[]) {
            Ok(()) => return Ok(Some(address)),
            Err(Error::NoAcknowledge(_)) => defmt::info!("No ACK at 0x{=u8:02x}", address),
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn draw<T: hal::i2c::Instance>(bus: &mut I2c<'_, T, hal::mode::Blocking>, address: u8) -> Result<(), Error> {
    // Command stream: display off, 64 rows, internal charge pump, horizontal
    // addressing, normal orientation and contrast, then cover the whole RAM.
    bus.blocking_write(
        address,
        &[
            0x00, 0xae, 0xd5, 0x80, 0xa8, 0x3f, 0xd3, 0x00, 0x40, 0x8d, 0x14, 0x20, 0x00, 0xa1, 0xc8, 0xda, 0x12, 0x81,
            0x7f, 0xd9, 0xf1, 0xdb, 0x40, 0xa4, 0xa6, 0x2e, 0x21, 0x00, 0x7f, 0x22, 0x00, 0x07,
        ],
    )?;

    // SSD1306 bytes represent vertical groups of eight pixels. Write every
    // page, including the black background, before turning the panel on.
    let mut page = [0u8; 129];
    page[0] = 0x40;
    for p in 0..8 {
        for x in 0..128 {
            let mut byte = 0;
            for bit in 0..8 {
                if pixel(x, p * 8 + bit) {
                    byte |= 1 << bit;
                }
            }
            page[x as usize + 1] = byte;
        }
        bus.blocking_write(address, &page)?;
    }
    bus.blocking_write(address, &[0x00, 0xaf])
}

fn pixel(x: i32, y: i32) -> bool {
    let dx = x - 63;
    let dy = y - 31;
    let radius_squared = dx * dx + dy * dy;
    let outline = (22 * 22..=24 * 24).contains(&radius_squared);
    let eyes = (dx - 8).abs() <= 2 && (dy + 7).abs() <= 2 || (dx + 8).abs() <= 2 && (dy + 7).abs() <= 2;
    let smile = dy >= 4 && dy <= 13 && (11 * 11..=13 * 13).contains(&radius_squared);
    outline || eyes || smile
}

fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
