//! Do real `lr.w`/`sc.w` atomics work on the CH569's QingKe V3A core?
//!
//! The HAL enables `unsafe-trust-wch-atomics`, so `core::sync::atomic` compiles
//! to LR/SC loops. embassy-executor's run queue is a lock-free CAS stack, and
//! tasks silently failed to run, which is what this checks.
//!
//! Progress is staged into RAMX so a hang points at the operation that stuck.
//!
//! ```console
//! cargo build --release --no-default-features --bin atomic_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 8
//! ```

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

static COUNTER: AtomicU32 = AtomicU32::new(0);
static SLOT: AtomicPtr<u32> = AtomicPtr::new(core::ptr::null_mut());

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());
    scratch(0, 0xa701_0001);

    // fetch_add: a stuck `sc.w` shows up as progress that stops early.
    for i in 0..10_000u32 {
        COUNTER.fetch_add(1, Ordering::Relaxed);
        scratch(1, i + 1);
    }
    scratch(2, COUNTER.load(Ordering::Relaxed));

    // Uncontended CAS must succeed.
    match COUNTER.compare_exchange(10_000, 20_000, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => scratch(3, 0x0cce_0001),
        Err(_) => scratch(3, 0x0cce_0002),
    }
    // Stale expected value must fail.
    match COUNTER.compare_exchange(10_000, 30_000, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => scratch(4, 0x0cce_0003),
        Err(v) => scratch(4, 0x0cce_0000 | v),
    }
    scratch(5, COUNTER.load(Ordering::Relaxed));
    scratch(6, 0xdead_0001);

    // AtomicPtr CAS, the operation embassy's run queue is built on.
    let mut a = 0x11u32;
    let mut b = 0x22u32;
    let pa: *mut u32 = &mut a;
    let pb: *mut u32 = &mut b;
    SLOT.store(pa, Ordering::SeqCst);
    let r1 = SLOT.compare_exchange(pa, pb, Ordering::SeqCst, Ordering::SeqCst).is_ok();
    let r2 = SLOT.compare_exchange(pa, pb, Ordering::SeqCst, Ordering::SeqCst).is_ok();
    scratch(7, (r1 as u32) | ((r2 as u32) << 1) | ((SLOT.load(Ordering::SeqCst) == pb) as u32) << 2);
    scratch(8, 0xdead_0002);

    loop {
        core::hint::spin_loop();
    }
}
