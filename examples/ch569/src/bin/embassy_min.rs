//! Minimal executor trace over RTT: where does the CH569 executor stop?

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::info;
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

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

#[embassy_executor::task]
async fn t1() {
    info!("t1 entry");
    let mut n = 0u32;
    loop {
        n = n.wrapping_add(1);
        if n % 100_000 == 0 {
            info!("t1 tick {}", n);
        }
        Yield.await;
    }
}

#[embassy_executor::task]
async fn t3(i: usize) {
    info!("t3 entry arg={}", i);
    loop {
        Yield.await;
    }
}

#[embassy_executor::task]
async fn t2() {
    info!("t2 entry");
    loop {
        Yield.await;
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());
    info!("hal init");

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };
    info!("executor built");

    executor.run(move |spawner| {
        info!("init closure");
        info!("spawning");
        spawner.spawn(t3(7).unwrap());
        info!("spawn t3 done");
        spawner.spawn(t1().unwrap());
        info!("spawn t1 done");
        spawner.spawn(t2().unwrap());
        info!("spawn t2 done");
        info!("init done");
    })
}
