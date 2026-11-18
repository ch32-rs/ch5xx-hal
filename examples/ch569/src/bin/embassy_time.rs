//! Embassy executor on the CH569 SysTick time driver: blink and `Timer` jitter.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::gpio::{Level, Output, Speed};
use defmt::info;
use defmt_rtt as _;
use embassy_time::{Duration, Instant, Timer};
use qingke::register::mtvec::{self, TrapMode};
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    unsafe {
        core::ptr::write_volatile(SCRATCH.add(12), 0xdead_0bad);
        if let Some(l) = info.location() {
            core::ptr::write_volatile(SCRATCH.add(13), l.line());
            core::ptr::write_volatile(SCRATCH.add(14), l.column());
        }
    }
    loop {}
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

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

/// Always-ready future: keeps the executor from idling into WFE, which cuts
/// the WCH-Link off from the core on this die.
struct Yield;

impl core::future::Future for Yield {
    type Output = ();
    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        // Pending, not Ready: the task has to give the executor its turn.
        cx.waker().wake_by_ref();
        core::task::Poll::Pending
    }
}

#[embassy_executor::task]
async fn keepalive() {
    let mut n = 0u32;
    loop {
        n = n.wrapping_add(1);
        scratch(9, n);
        scratch(11, Instant::now().as_millis() as u32);
        Yield.await;
    }
}

#[embassy_executor::task]
async fn blink(mut led0: Output<'static>, mut led1: Output<'static>) {
    scratch(7, 1);
    let mut n: u32 = 0;
    loop {
        scratch(12, 1);
        led0.toggle();
        led1.toggle();
        n += 1;
        scratch(1, n);
        scratch(12, 2);
        scratch(2, Instant::now().as_millis() as u32);
        scratch(12, 3);
        info!("toggle {} at {} ms", n, Instant::now().as_millis());
        scratch(12, 4);
        Timer::after(Duration::from_millis(500)).await;
        scratch(12, 5);
    }
}

#[embassy_executor::task]
async fn measure() {
    scratch(8, 1);
    let mut n: u32 = 0;
    loop {
        let start = Instant::now();
        scratch(13, 1);
        Timer::after(Duration::from_millis(50)).await;
        scratch(13, 2);
        let us = start.elapsed().as_micros() as u32;
        n += 1;
        scratch(3, n);
        scratch(4, us);
        if n == 1 || us < unsafe { core::ptr::read_volatile(SCRATCH.add(5)) } {
            scratch(5, us);
        }
        if us > unsafe { core::ptr::read_volatile(SCRATCH.add(6)) } {
            scratch(6, us);
        }
        info!("elapsed {} us", us);
        Timer::after(Duration::from_millis(950)).await;
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    // A silent fault lands in qingke-rt's `j .` and looks like a hung executor.
    unsafe { mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct) };

    scratch(0, 0x5449_4d45); // "TIME"
    for i in 1..16 {
        scratch(i, 0);
    }
    scratch(5, u32::MAX);

    info!("embassy sysclk = {} Hz", hal::rcc::clocks().sysclk.0);
    scratch(10, 1);

    // Active low: start both LEDs off.
    let led0 = Output::new(p.PB24, Level::High, Speed::Low);
    let led1 = Output::new(p.PB22, Level::High, Speed::Low);
    scratch(10, 2);

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };
    scratch(10, 3);

    executor.run(move |spawner| {
        spawner.spawn(blink(led0, led1).unwrap());
        scratch(10, 4);
        spawner.spawn(measure().unwrap());
        scratch(10, 5);
        spawner.spawn(keepalive().unwrap());
        scratch(10, 6);
    })
}
