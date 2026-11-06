//! Animate a test pattern on the SSD1306 through the interrupt-driven I2C master.
//!
//! The panel answers at `0x3C`. SCL is PB13 and SDA is PB12, and both lines need
//! an external pull-up. The bus runs at 400 kHz from the 24 MHz PLL, the same
//! wiring as `i2c_oled`.
//!
//! Three async frames are sent per step: the init sequence, a column/page
//! window, and a 1025-byte frame write. The last one starts with the SSD1306
//! data control byte `0x40`, then a 1024-byte frame buffer, which exercises the
//! interrupt-driven byte path in both directions of the address byte and the
//! full frame length. The buffer alternates between vertical bars and a diagonal
//! pattern.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::i2c::{Config, I2c, InterruptHandler};
use hal::mode::Async;
use hal::peripherals::I2C0;
use hal::rcc::Config as ClockConfig;
use hal::time::Hertz;
use panic_halt as _;
use qingke_rt::entry;

hal::bind_interrupts!(struct Irqs {
    I2C0 => InterruptHandler<I2C0>;
});

/// 7-bit address the scan found.
const ADDRESS: u8 = 0x3c;
const WIDTH: usize = 128;
const PAGES: usize = 8;

/// Commands, with the leading `0x00` that marks the rest of the write as
/// commands. The charge pump has to be on or the panel stays dark.
const INIT: &[u8] = &[
    0x00, 0xAE, // display off
    0xD5, 0x80, // clock divide
    0xA8, 0x3F, // multiplex ratio, 64 rows
    0xD3, 0x00, // display offset
    0x40, // start line 0
    0x8D, 0x14, // charge pump on
    0x20, 0x00, // horizontal addressing
    0xA1, // segment remap
    0xC8, // scan from COM63
    0xDA, 0x12, // COM pins
    0x81, 0xCF, // contrast
    0xD9, 0xF1, // pre-charge
    0xDB, 0x40, // VCOMH
    0xA4, // use the frame buffer
    0xA6, // normal, not inverted
    0xAF, // display on
];

/// Column and page window covering the whole panel. Sent before every frame so a
/// short write cannot leave the pointer mid-page.
const WINDOW: &[u8] = &[0x00, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x07];

/// `0x40` selects the data stream; without it the panel reads pixel bytes as
/// control bytes and then as commands, leaving the display RAM untouched.
const DATA: usize = 0x40;

static mut FRAME: [u8; 1 + WIDTH * PAGES] = {
    let mut frame = [0; 1 + WIDTH * PAGES];
    frame[0] = DATA as u8;
    frame
};

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: ClockConfig::HSI_PLL_24MHZ,
    });

    let bus = I2c::new::<0>(p.I2C0, Irqs, p.PB13, p.PB12, Config::new(Hertz(400_000))).unwrap();
    let frame = unsafe { &mut *core::ptr::addr_of_mut!(FRAME) };

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(run(bus, frame).unwrap());
    })
}

#[embassy_executor::task]
async fn run(mut bus: I2c<'static, I2C0, Async>, frame: &'static mut [u8]) -> ! {
    let mut bars = true;
    loop {
        for page in 0..PAGES {
            for column in 0..WIDTH {
                frame[1 + page * WIDTH + column] = if bars {
                    if (column / 8 + page) % 2 == 0 {
                        0xFF
                    } else {
                        0x0F
                    }
                } else {
                    let bit = (column + page * 8) % 8;
                    1 << bit
                };
            }
        }

        if let Err(error) = bus.write(ADDRESS, INIT).await {
            defmt::error!("oled init failed: {}", code(error));
            halt();
        }
        if let Err(error) = bus.write(ADDRESS, WINDOW).await {
            defmt::error!("oled window failed: {}", code(error));
            halt();
        }
        if let Err(error) = bus.write(ADDRESS, frame).await {
            defmt::error!("oled frame failed: {}", code(error));
            halt();
        }
        defmt::info!("oled async frame written");
        bars = !bars;
        embassy_time::Timer::after_millis(500).await;
    }
}

fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// Compact tag for the async error, which is not defmt-formatted.
fn code(error: hal::i2c::Error) -> u8 {
    match error {
        hal::i2c::Error::Bus => 1,
        hal::i2c::Error::ArbitrationLoss => 2,
        hal::i2c::Error::NoAcknowledge(_) => 3,
        hal::i2c::Error::Overrun => 4,
        hal::i2c::Error::Timeout => 5,
        hal::i2c::Error::Address => 6,
        hal::i2c::Error::ZeroLength => 7,
        _ => 8,
    }
}
