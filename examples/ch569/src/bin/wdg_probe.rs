//! Watchdog reset on CH569: cause flag, `GLOB_RESET_KEEP`, control register.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::watchdog::{reset_cause, ResetCause, Watchdog};
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
// Writable RAMX on this board ends at 0x20030000.
const BOOT_MAGIC: *mut u32 = 0x2002_7000 as *mut u32;
const MAGIC: u32 = 0x5744_474f; // "WDGO"

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn raw_boot_stat() -> u32 {
    let flag = hal::pac::SYSTEM_CONTROL.rst_boot_stat().read().0;
    flag as u32
}

fn raw_reset_keep() -> u8 {
    hal::pac::SYSTEM_CONTROL.glob_reset_keep().read().0
}

fn raw_set_reset_keep(v: u8) {
    hal::pac::SYSTEM_CONTROL.glob_reset_keep().write(|w| w.0 = v);
}

fn raw_wdog_ctrl() -> u8 {
    hal::pac::SYSTEM_CONTROL.rst_wdog_ctrl().read().0
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    scratch(0, u32::from_le_bytes(*b"WDGO"));
    scratch(1, unsafe { core::ptr::read_volatile(BOOT_MAGIC) });
    scratch(2, raw_reset_keep() as u32);
    scratch(3, raw_boot_stat());

    if unsafe { core::ptr::read_volatile(BOOT_MAGIC) } == MAGIC {
        // Boot B: the watchdog was armed on the boot that wrote the magic.
        let cause = reset_cause();
        unsafe { core::ptr::write_volatile(BOOT_MAGIC, 0) };
        let code = match cause {
            ResetCause::Software => 0,
            ResetCause::PowerOn => 1,
            ResetCause::Watchdog => 2,
            ResetCause::Pin => 3,
            ResetCause::ShutdownWake => 5,
            ResetCause::Other(v) => 0x100 | v as u32,
        };
        scratch(4, code);
        // RESET_KEEP survives every reset but a power-on one, so it can tell a
        // misreported watchdog reset from a real power cycle.
        scratch(5, (raw_reset_keep() == 0xa5) as u32);
        scratch(6, if raw_reset_keep() == 0xa5 { 0x5041_5353 } else { 0x4641_494c }); // PASS/FAIL
        loop {
            core::hint::spin_loop();
        }
    }

    // Boot A: arm once and wait. A watchdog reset that arrives without the
    // magic means RAMX did not survive, so stop rather than re-arm forever.
    if reset_cause() == ResetCause::Watchdog {
        scratch(6, 0x4E4F_584D); // "NOXM"
        loop {
            core::hint::spin_loop();
        }
    }
    unsafe { core::ptr::write_volatile(BOOT_MAGIC, MAGIC) };
    raw_set_reset_keep(0xa5);
    let wdg = Watchdog::new(p.SYSTEM_CONTROL, 0);
    wdg.enable_reset();
    // Read the control register back inside the same boot: a later reset clears it.
    scratch(7, raw_wdog_ctrl() as u32);
    scratch(8, 1); // armed
    loop {
        core::hint::spin_loop();
    }
}
