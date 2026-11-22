//! HAL clock switch to 120 MHz with no output device, staged in RAMX.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::rcc;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

#[entry]
fn main() -> ! {
    let _ = hal::init(rcc::Config::new(rcc::Osc::Hse32, rcc::Sys::Pll(4)).into());

    unsafe {
        core::ptr::write_volatile(SCRATCH, 0xAA55_0003);
        core::ptr::write_volatile(SCRATCH.add(1), rcc::clocks().sysclk.0);
        core::ptr::write_volatile(SCRATCH.add(2), hal::delay::measured_sysclk().0);
    }

    // One wall-second of busy-waiting must cover about Fsys SysTick counts.
    let start = ch5xx_hal::pac::SYSTICK.cntl().read().cntl();
    hal::delay::Delay.delay_ms(1000);
    let end = ch5xx_hal::pac::SYSTICK.cntl().read().cntl();
    unsafe { core::ptr::write_volatile(SCRATCH.add(4), start.wrapping_sub(end)) };

    let mut n: u32 = 0;
    loop {
        n = n.wrapping_add(1);
        unsafe { core::ptr::write_volatile(SCRATCH.add(3), n) };
    }
}
