//! Does the SysTick time driver's alarm reach an embassy task on CH569?

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use embassy_time::{Duration, Instant, Timer};
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
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

/// Always ready, so a frozen executor and a healthy one look different.
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
    let mut n = 0u32;
    loop {
        scratch(6, 1);
        let start = Instant::now();
        Timer::after(Duration::from_millis(100)).await;
        scratch(6, 2);
        let us = start.elapsed().as_micros() as u32;
        n += 1;
        scratch(2, n);
        scratch(3, us);
        if n == 1 || us < unsafe { core::ptr::read_volatile(SCRATCH.add(4)) } {
            scratch(4, us);
        }
        if us > unsafe { core::ptr::read_volatile(SCRATCH.add(5)) } {
            scratch(5, us);
        }
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x544d_5250); // "TMRP"
    for i in 1..8 {
        scratch(i, 0);
    }
    scratch(4, u32::MAX);

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(keeper().unwrap());
        spawner.spawn(ticker().unwrap());
        scratch(7, 1);
    })
}
