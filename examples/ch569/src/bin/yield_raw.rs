//! Does the embassy executor keep polling always-ready tasks on CH569?
//!
//! No defmt, no timer: three tasks each bump a RAMX counter and yield.
//!
//! ```console
//! cargo build --release --bin yield_raw
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 6
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke::register::mtvec::{self, TrapMode};
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

core::arch::global_asm!(
    r#"
    .section .text
    .global _trap_dump
    .align 2
_trap_dump:
    csrr t0, mcause
    bgez t0, 2f
    andi t1, t0, 0xff
    slli t1, t1, 2
    lw   t2, 0(t1)
    jr   t2
2:
    la   t1, 0x20022000
    sw   t0, 0x00(t1)
    csrr t2, mepc
    sw   t2, 0x04(t1)
    csrr t2, mtval
    sw   t2, 0x08(t1)
    sw   sp, 0x0c(t1)
    sw   gp, 0x10(t1)
    sw   tp, 0x14(t1)
    sw   ra, 0x18(t1)
    sw   a0, 0x1c(t1)
    sw   a1, 0x20(t1)
    sw   a2, 0x24(t1)
    sw   a3, 0x28(t1)
    sw   a4, 0x2c(t1)
    sw   a5, 0x30(t1)
    sw   a6, 0x34(t1)
    sw   a7, 0x38(t1)
    sw   t0, 0x3c(t1)
    sw   t1, 0x40(t1)
    sw   t2, 0x44(t1)
    sw   t3, 0x48(t1)
    sw   t4, 0x4c(t1)
    sw   t5, 0x50(t1)
    sw   t6, 0x54(t1)
    sw   s0, 0x58(t1)
    sw   s1, 0x5c(t1)
    sw   s2, 0x60(t1)
    sw   s3, 0x64(t1)
    sw   s4, 0x68(t1)
    sw   s5, 0x6c(t1)
    sw   s6, 0x70(t1)
    sw   s7, 0x74(t1)
    sw   s8, 0x78(t1)
    sw   s9, 0x7c(t1)
    sw   s10, 0x80(t1)
    sw   s11, 0x84(t1)
1:  j 1b
"#
);

extern "C" {
    fn _trap_dump();
}

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

/// `Pending` once (after re-waking), then `Ready`, so the loop advances.
struct Yield(bool);

impl core::future::Future for Yield {
    type Output = ();
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        if self.0 {
            core::task::Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    }
}

macro_rules! spinner {
    ($name:ident, $slot:expr) => {
        #[embassy_executor::task]
        async fn $name() {
            let mut n = 0u32;
            loop {
                n = n.wrapping_add(1);
                scratch($slot, n);
                Yield(false).await;
            }
        }
    };
}

spinner!(spinner_a, 1);
spinner!(spinner_b, 2);
spinner!(spinner_c, 3);

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    // Zero the dump area so a later read can tell "no fault" from stale data.
    unsafe {
        let dump = 0x2002_2000 as *mut u32;
        for i in 0..40 {
            core::ptr::write_volatile(dump.add(i), 0);
        }
    }
    // Zero both executor rings: events at 0x20021300, anomalies at 0x20021a00.
    unsafe {
        let ring = 0x2002_1300 as *mut u32;
        for i in 0..((0x2002_1f00 - 0x2002_1300) / 4) {
            core::ptr::write_volatile(ring.add(i), 0);
        }
    }
    unsafe { mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct) };

    scratch(0, 0x5949_454c); // "YIEL"
    for i in 1..6 {
        scratch(i, 0);
    }

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(spinner_a().unwrap());
        spawner.spawn(spinner_b().unwrap());
        spawner.spawn(spinner_c().unwrap());
        scratch(4, 1);
    })
}
