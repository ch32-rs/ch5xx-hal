//! Watch CH569 SysTick across the zero crossing with no ISR and no writes.
//!
//! CMP is loaded once, STIE stays off, and CNT plus CNTIF are sampled with a
//! gap that fits 12 samples into one period. A reload shows up as CNT jumping
//! back to ~CMP; a free-running counter shows up as a huge value.
//!
//! ```console
//! cargo build --release --no-default-features --bin crossing_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 30
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const PERIOD: u32 = 100_000;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

#[inline(never)]
fn spin(n: u32) {
    let mut i = 0;
    while i < n {
        i = i.wrapping_add(1);
        core::hint::spin_loop();
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x4352_4f53); // "CROS"
    for i in 1..30 {
        scratch(i, 0);
    }

    let stk = &pac::SYSTICK;
    stk.ctlr().modify(|w| {
        w.set_ste(false);
        w.set_stie(false);
    });
    stk.cntfg().modify(|w| w.set_cntif(false));
    stk.cntl().write(|w| w.set_cntl(0));
    stk.cnth().write(|w| w.set_cnth(0));
    stk.cmplr().write(|w| w.set_cmpl(PERIOD));
    stk.cmphr().write(|w| w.set_cmph(0));

    // One store, as the vendor does it, then never touch the registers again.
    stk.ctlr().write(|w| {
        w.set_streload(true);
        w.set_stclk(true);
        w.set_ste(true);
    });

    for k in 0..24 {
        scratch(1 + 2 * k, stk.cntl().read().cntl());
        scratch(2 + 2 * k, stk.cntfg().read().0);
        spin(3_000);
    }

    scratch(26, stk.cntl().read().cntl());
    scratch(27, stk.cnth().read().cnth());
    scratch(28, stk.cntfg().read().0);
    scratch(29, 0xdead_0001);

    loop {
        core::hint::spin_loop();
    }
}
