//! Read the two user buttons on the CH585M-R1-1v1 board and log every change.
//!
//! S1 is PB22 and S2 is PB23. Both buttons short the pin to GND, so they are
//! inputs with the internal pull-up and read low when pressed.
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
use hal::gpio::{Input, Pull};
use panic_halt as _;
use qingke_rt::entry;

/// Sample interval, in milliseconds.
const SAMPLE_MS: u32 = 10;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let s1 = Input::new(p.PB22, Pull::Up);
    let s2 = Input::new(p.PB23, Pull::Up);
    let mut delay = Delay;

    defmt::info!("S1=PB22 S2=PB23, pull-up, sampling every {} ms", SAMPLE_MS);

    let mut s1_low = s1.is_low();
    let mut s2_low = s2.is_low();
    defmt::info!(
        "initial levels: S1={=str} S2={=str}",
        if s1_low { "low" } else { "high" },
        if s2_low { "low" } else { "high" }
    );

    loop {
        let now1 = s1.is_low();
        if now1 != s1_low {
            s1_low = now1;
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
