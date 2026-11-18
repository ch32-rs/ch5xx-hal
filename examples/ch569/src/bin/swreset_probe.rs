//! `watchdog::software_reset()` on CH569: the cause flag and the reset keep.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::watchdog::{reset_cause, ResetCause};
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const BOOT_MAGIC: *mut u32 = 0x2002_7000 as *mut u32;
const MAGIC: u32 = 0x5352_5753; // "SWRS"
const KEEP: u8 = 0x5a;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn raw_boot_stat() -> u8 {
    hal::pac::SYSTEM_CONTROL.rst_boot_stat().read().0
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, u32::from_le_bytes(*b"SWRS"));
    scratch(1, unsafe { core::ptr::read_volatile(BOOT_MAGIC) });
    scratch(3, raw_boot_stat() as u32);

    let powered_on = raw_boot_stat() & 0x03 == 0x01;
    if unsafe { core::ptr::read_volatile(BOOT_MAGIC) } == MAGIC && !powered_on {
        let cause = reset_cause();
        unsafe { core::ptr::write_volatile(BOOT_MAGIC, 0) };
        let keep = hal::pac::SYSTEM_CONTROL.glob_reset_keep().read().0;
        scratch(2, keep as u32);
        scratch(4, matches!(cause, ResetCause::Software) as u32);
        scratch(5, (keep == KEEP) as u32);
        scratch(6, if matches!(cause, ResetCause::Software) && keep == KEEP {
            0x5041_5353 // PASS
        } else {
            0x4641_494c // FAIL
        });
        loop {
            core::hint::spin_loop();
        }
    }

    unsafe { core::ptr::write_volatile(BOOT_MAGIC, MAGIC) };
    hal::pac::SYSTEM_CONTROL.glob_reset_keep().write(|w| w.0 = KEEP);
    scratch(7, 1); // armed
    hal::watchdog::software_reset()
}
