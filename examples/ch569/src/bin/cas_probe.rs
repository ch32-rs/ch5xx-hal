//! Does the `lr.w.aq` + `sc.w.rl` CAS actually store on CH569?

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const N: u32 = 50_000;

static ACQREL: AtomicU32 = AtomicU32::new(0);
static SEQ: AtomicU32 = AtomicU32::new(0);
static RELAXED: AtomicU32 = AtomicU32::new(0);
static PTR: AtomicU32 = AtomicU32::new(0);

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn bump(a: &AtomicU32, ord: Ordering) {
    let mut cur = a.load(Ordering::Relaxed);
    loop {
        match a.compare_exchange_weak(cur, cur.wrapping_add(1), ord, Ordering::Relaxed) {
            Ok(_) => return,
            Err(actual) => cur = actual,
        }
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x4341_5350); // "CASP"

    for _ in 0..N {
        bump(&ACQREL, Ordering::AcqRel);
    }
    scratch(1, ACQREL.load(Ordering::Relaxed));

    for _ in 0..N {
        bump(&SEQ, Ordering::SeqCst);
    }
    scratch(2, SEQ.load(Ordering::Relaxed));

    for _ in 0..N {
        bump(&RELAXED, Ordering::Relaxed);
    }
    scratch(3, RELAXED.load(Ordering::Relaxed));

    // The run-queue shape: store the payload, then publish with a CAS.
    for i in 0..N {
        let old = PTR.load(Ordering::Relaxed);
        loop {
            match PTR.compare_exchange_weak(old, old.wrapping_add(1), Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => {
                    scratch(4, actual); // unexpected contention (single thread)
                    core::hint::spin_loop();
                    break;
                }
            }
        }
        let _ = i;
    }
    scratch(5, PTR.load(Ordering::Relaxed));
    scratch(6, 0xcafe_0001);

    loop {
        core::hint::spin_loop();
    }
}
