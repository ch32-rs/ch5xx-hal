//! Read the user buttons on the CH585M-R1-1v1 board and show S1 on the LEDs.
//!
//! S1 is PB22 and S2 is PB23. Both buttons short the pin to GND, so they are
//! inputs with the internal pull-up and read low when pressed. LED0 is PB16 and
//! LED1 is PB17. While S1 is released PB16 is high and PB17 is low; pressing S1
//! swaps the two outputs. The board lights one of those levels, so the lit LED
//! moves with the button either way.
//!
//! The pins are sampled every 10 ms through the blocking `delay`. There is no
//! interrupt and no debounce: contact bounce can produce a burst of edges, and
//! a press shorter than one sample interval can be missed. That is enough to
//! confirm wiring; a real key driver would add debounce.
//!
//! `defmt` over RTT prints one line per level change.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::delay::Delay;
use hal::gpio::{Input, Level, Output, Pull, Speed};
use panic_halt as _;
use qingke_rt::entry;

/// Sample interval, in milliseconds.
const SAMPLE_MS: u32 = 10;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let s1 = Input::new(p.PB22, Pull::Up);
    let s2 = Input::new(p.PB23, Pull::Up);
    // Released is the idle state: PB16 high, PB17 low. A press swaps them.
    let mut led0 = Output::new(p.PB16, Level::High, Speed::Low);
    let mut led1 = Output::new(p.PB17, Level::Low, Speed::Low);
    let mut delay = Delay;

    defmt::info!(
        "S1=PB22 S2=PB23, LEDs PB16/PB17 follow S1, sampling every {} ms",
        SAMPLE_MS
    );

    let mut s1_low = s1.is_low();
    let mut s2_low = s2.is_low();
    show_s1(&mut led0, &mut led1, s1_low);
    defmt::info!(
        "initial levels: S1={=str} S2={=str}",
        if s1_low { "low" } else { "high" },
        if s2_low { "low" } else { "high" }
    );

    loop {
        let now1 = s1.is_low();
        if now1 != s1_low {
            s1_low = now1;
            show_s1(&mut led0, &mut led1, now1);
            defmt::info!("S1 (PB22) {=str}", if now1 { "low" } else { "high" });
        }

        let now2 = s2.is_low();
        if now2 != s2_low {
            s2_low = now2;
            defmt::info!("S2 (PB23) {=str}", if now2 { "low" } else { "high" });
        }

        delay.delay_ms(SAMPLE_MS);
    }
}

/// Drive the two LEDs from S1. Pressed and released are opposite output levels.
fn show_s1(led0: &mut Output<'_>, led1: &mut Output<'_>, pressed: bool) {
    if pressed {
        led0.set_low();
        led1.set_high();
    } else {
        led0.set_high();
        led1.set_low();
    }
}
