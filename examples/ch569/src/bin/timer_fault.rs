//! Timer probe with a full-register dump on exceptions.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use embassy_time::{Duration, Instant, Timer};
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
    csrr t0, mcause
    bgez t0, 2f
    # Interrupt: the vector table holds one address per cause code.
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

/// `Pending` once (after re-waking), then `Ready`.
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

#[embassy_executor::task]
async fn keeper() {
    let mut n = 0u32;
    loop {
        n = n.wrapping_add(1);
        scratch(1, n);
        Yield(false).await;
    }
}

#[embassy_executor::task]
async fn ticker() {
    loop {
        let start = Instant::now();
        Timer::after(Duration::from_millis(100)).await;
        let us = start.elapsed().as_micros() as u32;
        scratch(2, unsafe {
            core::ptr::read_volatile(SCRATCH.add(2)).wrapping_add(1)
        });
        scratch(3, us);
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    unsafe { mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct) };

    scratch(0, 0x5446_4c54); // "TFLT"
    for i in 1..4 {
        scratch(i, 0);
    }

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(keeper().unwrap());
        spawner.spawn(ticker().unwrap());
    })
}
