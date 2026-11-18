//! Embassy time driver in a bare loop: `Instant::now()` is a 64-bit division.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use embassy_time::Instant;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    let mut iter: u32 = 0;
    loop {
        let sp: u32;
        unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
        let t = Instant::now();
        unsafe {
            core::ptr::write_volatile(SCRATCH.add(1), sp);
            core::ptr::write_volatile(SCRATCH.add(2), t.as_micros() as u32);
            core::ptr::write_volatile(SCRATCH.add(3), (t.as_micros() >> 32) as u32);
            core::ptr::write_volatile(SCRATCH.add(4), iter);
        }
        iter = iter.wrapping_add(1);
    }
}
