//! Bare-metal 64-bit division loop: does `__udivdi3` fault on this core?
//!
//! Leaves the stack pointer and the last quotient/remainder in RAMX so a
//! halted-core read can show where it stopped.

#![no_std]
#![no_main]

use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

#[entry]
fn main() -> ! {
    let mut n: u64 = 0;
    let mut iter: u32 = 0;
    loop {
        let sp: u32;
        unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
        let d = core::hint::black_box(15_000_000u64);
        let q = core::hint::black_box(n) / d;
        let r = core::hint::black_box(n) % d;
        unsafe {
            core::ptr::write_volatile(SCRATCH.add(1), sp);
            core::ptr::write_volatile(SCRATCH.add(2), q as u32);
            core::ptr::write_volatile(SCRATCH.add(3), r as u32);
            core::ptr::write_volatile(SCRATCH.add(4), iter);
        }
        n = n.wrapping_add(0x1234_5678);
        iter = iter.wrapping_add(1);
    }
}
