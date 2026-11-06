//! Show the real-time clock on a 128x64 SSD1306.
//!
//! The panel answers at `0x3C`. SCL is PB13 and SDA is PB12, and both lines need
//! an external pull-up. The bus runs at 400 kHz from the 24 MHz PLL, the same
//! wiring as `i2c_oled`.
//!
//! The clock is loaded with 2026-11-06 12:00:00 and the panel is redrawn when the
//! RTC second changes: the date, `HH:MM:SS`, and a bar that fills across the
//! minute. The frame setup matches `i2c_oled`. A 128x32 panel will not match the
//! init sequence.

#![no_std]
#![no_main]

use core::convert::Infallible;

use ch5xx_hal as hal;
use defmt_rtt as _;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use hal::i2c::{Config, I2c};
use hal::mode::Blocking;
use hal::rcc::Config as ClockConfig;
use hal::rtc::{DateTime, Rtc};
use hal::time::Hertz;
use panic_halt as _;
use qingke_rt::entry;

/// 7-bit address the scan found.
const ADDRESS: u8 = 0x3c;
const WIDTH: i32 = 128;
const HEIGHT: i32 = 64;

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

/// One byte per eight vertical pixels, 128 columns by 8 pages.
static mut FRAME: [u8; (WIDTH as usize) * (HEIGHT as usize) / 8] = [0; (WIDTH as usize) * (HEIGHT as usize) / 8];

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: ClockConfig::HSI_PLL_24MHZ,
    });

    let mut rtc = Rtc::new(p.RTC);
    rtc.set_datetime(DateTime {
        year: 2026,
        month: 11,
        day: 6,
        hour: 12,
        minute: 0,
        second: 0,
    })
    .unwrap();

    let mut bus = I2c::new_blocking::<0>(p.I2C0, p.PB13, p.PB12, Config::new(Hertz(400_000))).unwrap();
    let frame = unsafe { &mut *core::ptr::addr_of_mut!(FRAME) };
    let mut screen = Screen { frame };

    if let Err(error) = bus.blocking_write(ADDRESS, INIT) {
        defmt::error!("oled init: {=u8}", code(error));
        halt();
    }
    defmt::info!("rtc oled");

    let mut shown = u8::MAX;
    loop {
        let now = rtc.datetime();
        if now.second == shown {
            core::hint::spin_loop();
            continue;
        }
        shown = now.second;
        defmt::info!(
            "{}-{}-{} {}:{}:{}",
            now.year,
            now.month,
            now.day,
            now.hour,
            now.minute,
            now.second
        );
        if let Err(error) = screen.draw_frame(&mut bus, now) {
            defmt::error!("oled frame: {=u8}", error);
            halt();
        }
    }
}

fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

struct Screen {
    frame: &'static mut [u8],
}

impl Screen {
    fn draw_frame<T: hal::i2c::Instance>(&mut self, bus: &mut I2c<T, Blocking>, now: DateTime) -> Result<(), u8> {
        self.frame.fill(0);
        let date = format_date(now);
        let time = format_time(now);
        let date_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
        let time_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let _ = Text::new(as_str(&date), Point::new(34, 16), date_style).draw(self);
        let _ = Text::new(as_str(&time), Point::new(24, 44), time_style).draw(self);
        // The bar grows with the second and is empty again at the next minute.
        let span = u32::from(now.second) * 124 / 59;
        if span > 0 {
            let _ = Rectangle::new(Point::new(2, 54), Size::new(span, 6))
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(self);
        }
        self.flush(bus)
    }

    fn flush<T: hal::i2c::Instance>(&self, bus: &mut I2c<T, Blocking>) -> Result<(), u8> {
        bus.blocking_write(ADDRESS, WINDOW).map_err(code)?;
        let mut page = [0u8; 129];
        page[0] = 0x40;
        for band in 0..8 {
            let start = band * WIDTH as usize;
            page[1..].copy_from_slice(&self.frame[start..start + WIDTH as usize]);
            bus.blocking_write(ADDRESS, &page).map_err(code)?;
        }
        Ok(())
    }
}

impl DrawTarget for Screen {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 || point.x >= WIDTH || point.y >= HEIGHT {
                continue;
            }
            let x = point.x as usize;
            let y = point.y as usize;
            let index = x + (y / 8) * WIDTH as usize;
            let mask = 1u8 << (y % 8);
            if color.is_on() {
                self.frame[index] |= mask;
            } else {
                self.frame[index] &= !mask;
            }
        }
        Ok(())
    }
}

impl OriginDimensions for Screen {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

fn format_date(now: DateTime) -> [u8; 10] {
    let mut buf = [0u8; 10];
    let year = now.year;
    buf[0] = b'0' + (year / 1000) as u8;
    buf[1] = b'0' + ((year / 100) % 10) as u8;
    buf[2] = b'0' + ((year / 10) % 10) as u8;
    buf[3] = b'0' + (year % 10) as u8;
    buf[4] = b'-';
    write_pair(&mut buf[5..7], now.month);
    buf[7] = b'-';
    write_pair(&mut buf[8..10], now.day);
    buf
}

fn format_time(now: DateTime) -> [u8; 8] {
    let mut buf = [0u8; 8];
    write_pair(&mut buf[0..2], now.hour);
    buf[2] = b':';
    write_pair(&mut buf[3..5], now.minute);
    buf[5] = b':';
    write_pair(&mut buf[6..8], now.second);
    buf
}

fn write_pair(slot: &mut [u8], value: u8) {
    slot[0] = b'0' + value / 10;
    slot[1] = b'0' + value % 10;
}

fn as_str(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap()
}

fn code(error: hal::i2c::Error) -> u8 {
    match error {
        hal::i2c::Error::Bus => 1,
        hal::i2c::Error::ArbitrationLoss => 2,
        hal::i2c::Error::NoAcknowledge(_) => 3,
        hal::i2c::Error::Overrun => 4,
        hal::i2c::Error::Timeout => 5,
        hal::i2c::Error::Address => 6,
        hal::i2c::Error::ZeroLength => 7,
        _ => 0,
    }
}
