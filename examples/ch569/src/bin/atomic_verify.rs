//! Verify every load/store/CAS on CH569 against its own re-read.
//!
//! Each round flips a word with a plain store, bumps it with a CAS, and swaps
//! a pointer; any store that does not read back is recorded with its round.
//!
//! ```console
//! cargo build --release --no-default-features --bin atomic_verify
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 20
//! ```

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

static X: AtomicU32 = AtomicU32::new(0);
static Y: AtomicU32 = AtomicU32::new(0);
static P: AtomicUsize = AtomicUsize::new(0);
static CANARY: [u32; 8] = [0; 8];

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x4154_4f4d); // "ATOM"
    for i in 1..20 {
        scratch(i, 0);
    }

    let mut store_bad = 0u32;
    let mut cas_bad = 0u32;
    let mut swap_bad = 0u32;
    let mut plain_bad = 0u32;

    let mut round = 0u32;
    while round < 5_000_000 {
        round = round.wrapping_add(1);

        // Plain store, verify by re-read.
        let v = round ^ 0x5a5a_5a5a;
        X.store(v, Ordering::Relaxed);
        let back = X.load(Ordering::Relaxed);
        if back != v {
            store_bad += 1;
            if store_bad == 1 {
                scratch(1, round);
                scratch(2, v);
                scratch(3, back);
            }
        }

        // CAS: the returned value and the stored value must agree.
        let old = Y.load(Ordering::Relaxed);
        match Y.compare_exchange(old, old.wrapping_add(1), Ordering::AcqRel, Ordering::Acquire) {
            Ok(prev) => {
                let now = Y.load(Ordering::Relaxed);
                if now != old.wrapping_add(1) || prev != old {
                    cas_bad += 1;
                    if cas_bad == 1 {
                        scratch(4, round);
                        scratch(5, old);
                        scratch(6, now);
                    }
                }
            }
            Err(actual) => {
                cas_bad += 1;
                if cas_bad == 1 {
                    scratch(4, round);
                    scratch(5, old);
                    scratch(6, actual);
                }
            }
        }

        // Swap a two-value pointer and read it back.
        let want = if round & 1 == 0 { &X as *const _ as usize } else { &Y as *const _ as usize };
        let was = P.load(Ordering::Relaxed);
        let got = P.swap(want, Ordering::AcqRel);
        let now = P.load(Ordering::Relaxed);
        if now != want || got != was {
            swap_bad += 1;
            if swap_bad == 1 {
                scratch(7, round);
                scratch(8, want as u32);
                scratch(9, now as u32);
                scratch(10, got as u32);
            }
        }

        // A plain array must keep its initial value.
        for (i, c) in CANARY.iter().enumerate() {
            if *c != 0 {
                plain_bad += 1;
                scratch(11, round);
                scratch(12, i as u32);
                scratch(13, *c);
            }
        }

        if round % 250_000 == 0 {
            scratch(14, round);
            scratch(15, store_bad);
            scratch(16, cas_bad);
            scratch(17, swap_bad);
        }
    }

    scratch(14, round);
    scratch(15, store_bad);
    scratch(16, cas_bad);
    scratch(17, swap_bad);
    scratch(18, 0xdead_0001);
    loop {
        core::hint::spin_loop();
    }
}

