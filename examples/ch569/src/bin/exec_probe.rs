//! Executor plus a full-register trap dump.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke::register::mtvec::{self, TrapMode};
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

core::arch::global_asm!(
    r#"
    .section .text
    .global _trap_dump
    .align 2
_trap_dump:
    la   t3, 0x20022000
    csrr t4, mcause
    sw   t4, 0x00(t3)
    csrr t4, mepc
    sw   t4, 0x04(t3)
    csrr t4, mtval
    sw   t4, 0x08(t3)
    sw   sp, 0x0c(t3)
    sw   gp, 0x10(t3)
    sw   tp, 0x14(t3)
    sw   ra, 0x18(t3)
    sw   a0, 0x1c(t3)
    sw   a1, 0x20(t3)
    sw   a2, 0x24(t3)
    sw   a3, 0x28(t3)
    sw   a4, 0x2c(t3)
    sw   a5, 0x30(t3)
    sw   a6, 0x34(t3)
    sw   a7, 0x38(t3)
    sw   t0, 0x3c(t3)
    sw   t1, 0x40(t3)
    sw   t2, 0x44(t3)
    sw   t3, 0x48(t3)
    sw   t4, 0x4c(t3)
    sw   t5, 0x50(t3)
    sw   t6, 0x54(t3)
    sw   s0, 0x58(t3)
    sw   s1, 0x5c(t3)
1:  j 1b
"#
);

extern "C" {
    fn _trap_dump();
}

struct Yield;

impl core::future::Future for Yield {
    type Output = ();
    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        cx.waker().wake_by_ref();
        core::task::Poll::Pending
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
                Yield.await;
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

    unsafe { mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct) };

    scratch(0, 0x4558_4543); // "CEXE"
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
