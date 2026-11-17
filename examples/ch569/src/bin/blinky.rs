//! Blink the three LEDs on the CH569 EVT board.
//!
//! LED0 is PB24, LED1 is PB22, LED2 is PB23; each is 3V3 -> 1K -> LED -> pin,
//! so the pin is active low. The three toggle at different rates so a single
//! dead pin is visible.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::delay::Delay;
use hal::gpio::{Level, Output, Speed};
use panic_halt as _;
use qingke_rt::entry;

/// Iteration counter, readable with a single memory read if RTT is unavailable.
#[unsafe(no_mangle)]
static mut TICKS: u32 = 0;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    defmt::info!("CH569 blinky, Fsys = {} Hz", hal::rcc::clocks().sysclk.0);

    let mut led0 = Output::new(p.PB24, Level::High, Speed::Low);
    let mut led1 = Output::new(p.PB22, Level::High, Speed::Low);
    let mut led2 = Output::new(p.PB23, Level::High, Speed::Low);

    let mut delay = Delay;
    let mut tick: u32 = 0;

    loop {
        led0.toggle();
        if tick % 2 == 0 {
            led1.toggle();
        }
        if tick % 4 == 0 {
            led2.toggle();
        }
        unsafe { TICKS = TICKS.wrapping_add(1) };
        tick = tick.wrapping_add(1);
        delay.delay_ms(125);
    }
}
