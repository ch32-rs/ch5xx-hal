//! Blink the two LEDs on the CH585 board.
//!
//! LED0 is PB16, LED1 is PB17, driven in opposite phase so a working build is
//! obvious even with only one LED populated.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use hal::delay::Delay;
use hal::gpio::{Level, Output, Speed};
use panic_halt as _;
use qingke_rt::entry;

/// Iteration counter, so progress can be observed with a single memory read
/// (every debugger read halts the core, so sampling `PB_OUT` freezes it).
#[unsafe(no_mangle)]
static mut BLINKS: u32 = 0;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut led0 = Output::new(p.PB16, Level::Low, Speed::Low);
    let mut led1 = Output::new(p.PB17, Level::High, Speed::Low);

    let mut delay = Delay;

    loop {
        led0.toggle();
        led1.toggle();
        unsafe { BLINKS = BLINKS.wrapping_add(1) };
        delay.delay_ms(500);
    }
}
