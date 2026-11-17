//! TMR0/TMR1/TMR2 interrupt path on CH569.
//!
//! Each timer raises `CYCLE_END` at 1 kHz; `Counter::wait` has to return once
//! per period through PFIC and the HAL handler (mask + wake). The spacing
//! between returns must match the period; a storm or a lost interrupt fails.
//!
//! ```console
//! cargo build --release --bin tmr_irq_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 16
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::Peri;
use embassy_time::Instant;
use hal::interrupt::typelevel::Binding;
use hal::timer::counter::Counter;
use hal::timer::{Instance, InterruptHandler, PeriodTicks};
use panic_halt as _;

hal::bind_interrupts!(struct Irqs0 { TMR0 => InterruptHandler<hal::peripherals::TMR0>; });
hal::bind_interrupts!(struct Irqs1 { TMR1 => InterruptHandler<hal::peripherals::TMR1>; });
hal::bind_interrupts!(struct Irqs2 { TMR2 => InterruptHandler<hal::peripherals::TMR2>; });

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const WAITS: u32 = 50;
const NOMINAL_US: u64 = 1000;
const TOLERANCE_PCT: u64 = 5;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

/// Returns 1 when every wait was spaced like one 1 kHz period.
async fn check<T, I>(peri: Peri<'_, T>, sysclk: u32, slot: usize, irq: I) -> u32
where
    T: Instance,
    I: Binding<T::Interrupt, InterruptHandler<T>> + Copy,
{
    let mut counter = Counter::new(peri, PeriodTicks::from_hz(sysclk, 1000).unwrap());
    // The first period is free-running, so start measuring from the second.
    counter.wait(irq).await;
    let mut last = Instant::now();
    let mut sum = 0u64;
    let mut min = u32::MAX;
    let mut max = 0u32;
    for _ in 0..WAITS {
        counter.wait(irq).await;
        let now = Instant::now();
        let delta = (now - last).as_micros() as u32;
        last = now;
        sum += delta as u64;
        min = min.min(delta);
        max = max.max(delta);
    }

    let mean = (sum / WAITS as u64) as u32;
    let mean_ok = (mean as u64) * 100 >= NOMINAL_US * (100 - TOLERANCE_PCT)
        && (mean as u64) * 100 <= NOMINAL_US * (100 + TOLERANCE_PCT);
    // A too-short wait means the source was not masked; a too-long one means a
    // period was lost.
    let spread_ok = min >= 300 && max <= 5000;

    scratch(slot * 4, mean);
    scratch(slot * 4 + 1, min);
    scratch(slot * 4 + 2, max);
    scratch(slot * 4 + 3, 0);
    let ok = (mean_ok && spread_ok) as u32;
    scratch(slot * 4 + 3, ok);
    ok
}

#[embassy_executor::main(entry = "qingke_rt::entry")]
async fn main(_spawner: embassy_executor::Spawner) {
    let p = hal::init(Default::default());
    let sysclk = hal::rcc::clocks().sysclk.0;

    scratch(0, u32::from_le_bytes(*b"TIRQ"));
    scratch(1, sysclk);
    scratch(2, 0);
    scratch(3, 0);

    let mut ok = 0;
    ok += check(p.TMR0, sysclk, 1, Irqs0).await;
    ok += check(p.TMR1, sysclk, 2, Irqs1).await;
    ok += check(p.TMR2, sysclk, 3, Irqs2).await;

    scratch(2, ok);
    scratch(3, if ok == 3 { 0x5041_5353 } else { 0x4641_494c }); // "PASS" / "FAIL"

    // Keep the core readable for the probe: no `wfi` after the results are in.
    loop {
        core::hint::spin_loop();
    }
}
