//! Scheduling probe: four spinner tasks plus a trap dump.
//!
//! Checks whether the thread-mode executor polls spawned tasks on CH569. A
//! private `mtvec` handler records any exception at 0x2002_2000, away from the
//! task counters at 0x2002_1000.
//!
//! ```console
//! cargo build --release --no-default-features --bin executor_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 6
//! probe-rs read --chip CH569 --probe ... b32 0x20022000 4
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt::info;
use defmt_rtt as _;
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
    sw   a0, 0x0c(t3)
    sw   sp, 0x10(t3)
    sw   ra, 0x14(t3)
1:
    j 1b
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

#[embassy_executor::task]
async fn spinner(i: usize) {
    info!("spinner {} entry", i);
    scratch(5, unsafe { core::ptr::read_volatile(SCRATCH.add(5)) } | (1 << i));
    let mut n = 0u32;
    loop {
        n = n.wrapping_add(1);
        scratch(i, n);
        Yield.await;
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());
    info!("hal init");
    unsafe { mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct) };

    scratch(0, 0x4558_4543); // "CEXE"
    for i in 1..5 {
        scratch(i, 0);
    }

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    info!("run");
    executor.run(move |spawner| {
        info!("init closure");
        spawner.spawn(spinner(1).unwrap());
        info!("spawned 1");
        spawner.spawn(spinner(2).unwrap());
        info!("spawned 2");
        spawner.spawn(spinner(3).unwrap());
        info!("spawned 3");
        spawner.spawn(spinner(4).unwrap());
        info!("init done");
    })
}
