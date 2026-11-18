//! GPIO input path and internal pulls on CH569; no jumpers needed.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::gpio::{Flex, Pull};
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

/// Reads `pin` with both pulls; returns `up * 2 + down`.
fn sample<P: hal::gpio::Pin>(pin: hal::Peri<'_, P>, delay: &mut hal::delay::Delay) -> u32 {
    let mut f = Flex::new(pin);
    f.set_as_input(Pull::Up);
    delay.delay_us(50);
    let up = f.is_high() as u32;
    f.set_as_input(Pull::Down);
    delay.delay_us(50);
    let down = f.is_high() as u32;
    up * 2 + down
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = hal::delay::Delay;

    scratch(0, u32::from_le_bytes(*b"GPIN"));
    scratch(1, 0);
    scratch(2, 0);
    scratch(3, 0);

    // LED pins first (their external circuit is known), then unused header pins.
    let mut floating = 0u32;
    let mut results = 0u32;

    macro_rules! run {
        ($id:expr, $($pin:expr),*) => {{
            let r = sample($($pin),*, &mut delay);
            scratch(4 + $id * 2, (1 << 16) | $id as u32);
            scratch(5 + $id * 2, r);
            if r == 2 { floating += 1; }
            results |= r << ($id * 2);
        }};
    }
    run!(0, p.PB24);
    run!(1, p.PB22);
    run!(2, p.PB23);
    run!(3, p.PB15);
    run!(4, p.PB14);
    run!(5, p.PA0);
    run!(6, p.PA1);
    run!(7, p.PB0);

    scratch(1, floating);
    // Any pin that followed the pull proves the input path and both pulls work.
    let ok = (floating > 0) as u32;
    scratch(2, if ok == 1 { 0x5041_5353 } else { 0x4641_494c }); // "PASS" / "FAIL"
    scratch(3, results);

    loop {
        core::hint::spin_loop();
    }
}
