//! Bare-metal clock switch, no HAL: the vendor `SystemInit(120MHz)` sequence.
//!
//! Leaves the two clock registers and a running counter in RAMX so a single
//! `probe-rs read` after a reset shows whether the switch stuck and whether
//! the core still executes.

#![no_std]
#![no_main]

use panic_halt as _;
use qingke_rt::entry;

const SAFE_ACCESS: *mut u8 = 0x4000_1000 as *mut u8;
const CLK_PLL_DIV: *mut u8 = 0x4000_1008 as *mut u8;
const CLK_CFG_CTRL: *mut u8 = 0x4000_100A as *mut u8;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

#[entry]
fn main() -> ! {
    unsafe {
        core::ptr::write_volatile(SCRATCH, 0xAA55_0002);
        SAFE_ACCESS.write_volatile(0x57);
        SAFE_ACCESS.write_volatile(0xA8);
        CLK_PLL_DIV.write_volatile(0x40 | 4);
        CLK_CFG_CTRL.write_volatile(0x80 | 0x02);
        SAFE_ACCESS.write_volatile(0);
        for _ in 0..64 {
            core::hint::spin_loop();
        }
        core::ptr::write_volatile(SCRATCH.add(1), core::ptr::read_volatile(CLK_PLL_DIV) as u32);
        core::ptr::write_volatile(SCRATCH.add(2), core::ptr::read_volatile(CLK_CFG_CTRL) as u32);
    }

    let mut n: u32 = 0;
    loop {
        n = n.wrapping_add(1);
        unsafe { core::ptr::write_volatile(SCRATCH.add(3), n) };
    }
}
