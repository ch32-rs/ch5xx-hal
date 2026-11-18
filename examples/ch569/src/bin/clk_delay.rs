//! CH569 delay accuracy, measured into RAMX.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use defmt::info;
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    info!("nominal sysclk  = {} Hz", hal::rcc::clocks().sysclk.0);
    info!("measured sysclk = {} Hz", hal::delay::measured_sysclk().0);

    unsafe {
        core::ptr::write_volatile(SCRATCH.add(1), hal::rcc::clocks().sysclk.0);
        core::ptr::write_volatile(SCRATCH.add(2), hal::delay::measured_sysclk().0);
    }

    let mut delay = hal::delay::Delay;
    let mut n: u32 = 0;
    loop {
        let start = pac::SYSTICK.cntl().read().cntl();
        delay.delay_ms(1000);
        let end = pac::SYSTICK.cntl().read().cntl();
        unsafe {
            core::ptr::write_volatile(SCRATCH.add(3), start);
            core::ptr::write_volatile(SCRATCH.add(4), end);
            core::ptr::write_volatile(SCRATCH.add(5), start.wrapping_sub(end));
            core::ptr::write_volatile(SCRATCH.add(6), n);
        }
        n = n.wrapping_add(1);
    }
}
