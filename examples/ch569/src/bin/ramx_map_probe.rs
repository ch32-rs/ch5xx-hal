//! Map the usable RAMX window and read the `USER_MEM` option word.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

const ADDRS: [usize; 9] = [
    0x2000_0800, // RAMS, control
    0x2002_4000, // RAMX, wide window
    0x2002_c000,
    0x2002_e000,
    0x2002_f000,
    0x2002_f800,
    0x2003_0000,
    0x2003_1000,
    0x2002_2000, // back inside, after the out-of-range probe
];

const OPTION_WORDS: [usize; 4] = [0x8010, 0x8014, 0x8018, 0x801c];

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    unsafe {
        for i in 0..44 {
            core::ptr::write_volatile(SCRATCH.add(i), 0);
        }
        core::ptr::write_volatile(SCRATCH, u32::from_le_bytes(*b"RXMP"));

        for (i, a) in ADDRS.iter().enumerate() {
            let p = *a as *mut u32;
            let before = core::ptr::read_volatile(p);
            let pattern = 0x51a5_0000 | i as u32;
            core::ptr::write_volatile(p, pattern);
            let after = core::ptr::read_volatile(p);
            core::ptr::write_volatile(SCRATCH.add(8 + 3 * i), before);
            core::ptr::write_volatile(SCRATCH.add(9 + 3 * i), pattern);
            core::ptr::write_volatile(SCRATCH.add(10 + 3 * i), after);
        }

        for (i, a) in OPTION_WORDS.iter().enumerate() {
            let v = core::ptr::read_volatile(*a as *const u32);
            core::ptr::write_volatile(SCRATCH.add(36 + i), v);
        }
    }

    loop {
        core::hint::spin_loop();
    }
}
