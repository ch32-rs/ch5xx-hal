//! Bare-metal CH569 GPIO probe: no HAL, no output device.
//!
//! Writes PB_DIR/PB_OUT directly and leaves counters in RAMX so a single
//! `probe-rs read` after the run can show whether code executed.

#![no_std]
#![no_main]

use panic_halt as _;
use qingke_rt::entry;

const PB_DIR: *mut u32 = 0x4000_1060 as *mut u32;
const PB_OUT: *mut u32 = 0x4000_1068 as *mut u32;
const PB_CLR: *mut u32 = 0x4000_106C as *mut u32;
const LEDS: u32 = (1 << 24) | (1 << 22) | (1 << 23);

/// RAMX scratch, unused by this image (the linker keeps data in RAMS).
const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

#[entry]
fn main() -> ! {
    unsafe {
        core::ptr::write_volatile(SCRATCH, 0xAA55_0001);
        core::ptr::write_volatile(PB_DIR, core::ptr::read_volatile(PB_DIR) | LEDS);
        core::ptr::write_volatile(SCRATCH.add(1), core::ptr::read_volatile(PB_DIR));
    }

    let mut n: u32 = 0;
    loop {
        unsafe {
            core::ptr::write_volatile(PB_CLR, LEDS);
            for _ in 0..200_000 {
                core::arch::asm!("nop");
            }
            core::ptr::write_volatile(PB_OUT, LEDS);
            for _ in 0..200_000 {
                core::arch::asm!("nop");
            }
            n = n.wrapping_add(1);
            core::ptr::write_volatile(SCRATCH.add(2), n);
        }
    }
}
