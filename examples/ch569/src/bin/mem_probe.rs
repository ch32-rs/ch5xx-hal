//! Store-and-read-back integrity for RAMS, with interleaved RAMX writes.

#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU32, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const RAM_SCRATCH: *mut u32 = 0x2000_1000 as *mut u32;
const X_SCRATCH: *mut u32 = 0x2002_5000 as *mut u32;
const ROUNDS: u32 = 1_500_000;

static AMO_CELL: AtomicU32 = AtomicU32::new(0);

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn fail(phase: u32, round: u32, addr: u32, expected: u32, actual: u32) -> ! {
    scratch(1, phase);
    scratch(2, round);
    scratch(3, addr);
    scratch(4, expected);
    scratch(5, actual);
    scratch(6, 0xdead_0000 | phase);
    loop {
        core::hint::spin_loop();
    }
}

fn check(phase: u32, round: u32, addr: u32, expected: u32) {
    let actual = unsafe { read_volatile(addr as *const u32) };
    if actual != expected {
        fail(phase, round, addr, expected, actual);
    }
}

/// `mix` bit 0 interleaves a RAMX write, bit 1 an AMO.
fn phase(id: u32, mix: u32) {
    let mut local = [0u32; 4];
    let stack = local.as_mut_ptr() as u32;
    for round in 0..ROUNDS {
        let a = round ^ 0x1234_5678 ^ id;
        let b = !round ^ 0x0f0f_0f0f;
        unsafe {
            write_volatile(stack as *mut u32, a);
            write_volatile(RAM_SCRATCH, b);
            write_volatile((stack as *mut u32).add(1), b);
        }
        if mix & 1 != 0 {
            unsafe { write_volatile(X_SCRATCH, round) };
            unsafe { write_volatile(X_SCRATCH.add(1), a) };
        }
        if mix & 2 != 0 {
            let prev = AMO_CELL.fetch_or(1, Ordering::AcqRel);
            AMO_CELL.store(prev & !1, Ordering::Relaxed);
        }
        check(id, round, stack, a);
        check(id, round, RAM_SCRATCH as u32, b);
        check(id, round, stack + 4, b);
        if mix & 1 != 0 {
            check(id, round, X_SCRATCH as u32, round);
            check(id, round, X_SCRATCH as u32 + 4, a);
        }
        // Read the stack array through the compiler's own view too.
        if local[0] != a {
            fail(id, round, stack, a, local[0]);
        }
        if local[1] != b {
            fail(id, round, stack + 4, b, local[1]);
        }
    }
    scratch(7, 0xa5a5_0000 | id);
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x4d45_4d50); // "MEMP"
    unsafe {
        write_volatile(SCRATCH.add(1), 0);
        write_volatile(SCRATCH.add(2), 0);
        write_volatile(SCRATCH.add(3), 0);
        write_volatile(SCRATCH.add(4), 0);
        write_volatile(SCRATCH.add(5), 0);
        write_volatile(SCRATCH.add(6), 0);
        write_volatile(SCRATCH.add(7), 0);
        write_volatile(SCRATCH.add(8), read_volatile(0x4000_1005 as *const u8) as u32);
    }

    phase(1, 0); // stack + RAMS only
    phase(2, 1); // plus RAMX traffic
    phase(3, 3); // plus AMOs

    loop {
        core::hint::spin_loop();
    }
}
