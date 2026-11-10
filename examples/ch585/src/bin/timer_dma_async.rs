//! Interrupt-driven finite Timer DMA, including transfer cancellation.
#![no_std]
#![no_main]
use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::interrupt::typelevel::Binding;
use hal::timer::counter::Counter;
use hal::timer::input_capture::InputCapture;
use hal::timer::simple_pwm::SimplePwm;
use hal::timer::{CaptureEdge, DmaInstance, InterruptHandler, OutputPolarity, PeriodTicks, Repeat, TimerPin};
use hal::Peri;
use panic_halt as _;

#[no_mangle]
pub static mut ASYNC_RESULTS: [u32; 8] = [0; 8];
fn record(n: usize, value: u32) {
    unsafe {
        core::ptr::addr_of_mut!(ASYNC_RESULTS)
            .cast::<u32>()
            .add(n)
            .write_volatile(value)
    }
}
#[cfg(not(feature = "time-driver-tmr0"))]
hal::bind_interrupts!(struct Irqs0 { TMR0 => InterruptHandler<hal::peripherals::TMR0>; });
#[cfg(not(feature = "time-driver-tmr1"))]
hal::bind_interrupts!(struct Irqs1 { TMR1 => InterruptHandler<hal::peripherals::TMR1>; });
#[cfg(not(feature = "time-driver-tmr2"))]
hal::bind_interrupts!(struct Irqs2 { TMR2 => InterruptHandler<hal::peripherals::TMR2>; });
#[cfg(not(feature = "time-driver-tmr3"))]
hal::bind_interrupts!(struct Irqs3 { TMR3 => InterruptHandler<hal::peripherals::TMR3>; });

static TX: [hal::dma::StaticBuffer<16>; 4] = [const { hal::dma::StaticBuffer::new([19_500; 16]) }; 4];
static RX: [hal::dma::StaticBuffer<16>; 4] = [const { hal::dma::StaticBuffer::new([0; 16]) }; 4];
static CANCEL: [hal::dma::StaticBuffer<18>; 4] = [const { hal::dma::StaticBuffer::new([0; 18]) }; 4];
static TX_CANCEL: [hal::dma::StaticBuffer<16>; 4] = [const { hal::dma::StaticBuffer::new([19_500; 16]) }; 4];
async fn run<T: DmaInstance, P: TimerPin<T, R>, I: Binding<T::Interrupt, InterruptHandler<T>> + Copy, const R: u8>(
    mut peri: Peri<'static, T>,
    mut pin: Peri<'static, P>,
    irq: I,
    regs: hal::pac::tmr::TmrFull,
    id: usize,
) {
    record(1, 10 + id as u32);
    let period = PeriodTicks::new(78_000).unwrap();
    let mut counter = Counter::new(peri.reborrow(), period);
    counter.wait(irq).await;
    drop(counter);

    let mut pwm = SimplePwm::new::<R>(
        peri.reborrow(),
        pin.reborrow(),
        period,
        OutputPolarity::ActiveHigh,
        Repeat::Four,
    );
    let tx = TX[id].take().unwrap();
    let tr = pwm.write(tx).map_err(|_| ()).unwrap();
    let tx = tr.finish(irq).await.map_err(|_| ()).unwrap();
    assert!(regs.ctrl_dma().read().0 == 0 && regs.ctrl_mod().read().0 & 0x0c == 0);
    assert!(tx.len() == 16);
    drop(pwm);

    let mut cap = InputCapture::new::<R>(
        peri.reborrow(),
        pin.reborrow(),
        hal::gpio::Pull::Down,
        period,
        CaptureEdge::RisingToRising,
    );
    let rx = RX[id].take().unwrap();
    let mut tr = cap.read(rx).map_err(|_| ()).unwrap();
    let ok = tr.wait(irq).await.is_ok();
    let mut rx = tr.stop();
    assert!(ok && rx.iter().all(|v| *v == 0x020130b0));
    // Cancelling only wait must preserve the transfer owner. Explicit stop
    // returns the storage, which is then reused immediately in another DMA.
    for delay_us in [0, 200, 1500, 4500] {
        rx.fill(0xdeadbeef);
        let mut tr = cap.read(rx).map_err(|_| ()).unwrap();
        let _ = embassy_time::with_timeout(embassy_time::Duration::from_micros(delay_us), tr.wait(irq)).await;
        rx = tr.stop();
        rx.fill(0xa55aa55a);
        embassy_time::Timer::after_millis(2).await;
        assert!(rx.iter().all(|v| *v == 0xa55aa55a));
    }
    let cancel_storage = CANCEL[id].take().unwrap();
    cancel_storage[0] = 0x12345678;
    cancel_storage[17] = 0x87654321;
    let (guard_a, rest) = cancel_storage.split_at_mut(1);
    let (cancel, guard_b) = rest.split_at_mut(16);
    let cancelled_ptr = cancel.as_ptr();
    let mut tr = cap.read_circular(cancel).map_err(|_| ()).unwrap();
    // Cancellation drops the owner as well as its wait future. Static storage
    // remains reserved; only an explicit stop can hand it back to the caller.
    let cancelled = embassy_time::with_timeout(embassy_time::Duration::from_millis(2), async move {
        loop {
            let _ = tr.wait(irq).await;
            core::future::pending::<()>().await;
        }
    })
    .await
    .is_err();
    assert!(cancelled && regs.ctrl_dma().read().0 == 0 && regs.ctrl_mod().read().0 & 0x0c == 0);
    let snapshot = core::array::from_fn::<_, 16, _>(|i| unsafe { cancelled_ptr.add(i).read_volatile() });
    embassy_time::Timer::after_millis(3).await;
    assert!(snapshot
        .iter()
        .enumerate()
        .all(|(i, v)| *v == unsafe { cancelled_ptr.add(i).read_volatile() }));
    assert!(guard_a[0] == 0x12345678 && guard_b[0] == 0x87654321);
    drop(cap);
    // Cancellation of consuming finish must stop output as well as DMA.
    let mut pwm = SimplePwm::new::<R>(
        peri.reborrow(),
        pin.reborrow(),
        period,
        OutputPolarity::ActiveHigh,
        Repeat::Sixteen,
    );
    let tx = TX_CANCEL[id].take().unwrap();
    let ptr = tx.as_ptr();
    let tr = pwm.write(tx).map_err(|_| ()).unwrap();
    assert!(
        embassy_time::with_timeout(embassy_time::Duration::from_millis(2), tr.finish(irq),)
            .await
            .is_err()
    );
    assert!(regs.ctrl_dma().read().0 == 0 && regs.ctrl_mod().read().0 & 0x0c == 0);
    embassy_time::Timer::after_millis(2).await;
    assert!((0..16).all(|i| unsafe { ptr.add(i).read_volatile() == 19_500 }));
    drop(pwm);
    let mut counter = Counter::new(peri.reborrow(), period);
    assert!(
        embassy_time::with_timeout(embassy_time::Duration::from_millis(10), counter.wait(irq))
            .await
            .is_ok()
    );
    // No RTT writes between tests: debugger polling can halt the CPU long
    // enough for finite capture's stopped DMA to leave an overflowing FIFO.
    record(4 + id, 1);
}

#[embassy_executor::task]
async fn exercise() {
    let p = hal::init(hal::rcc::Config::HSE_PLL_78MHZ.into());
    #[cfg(feature = "time-driver-systick")]
    record(2, 0);
    #[cfg(feature = "time-driver-tmr0")]
    record(2, 1);
    #[cfg(feature = "time-driver-tmr1")]
    record(2, 2);
    #[cfg(feature = "time-driver-tmr2")]
    record(2, 3);
    #[cfg(feature = "time-driver-tmr3")]
    record(2, 4);
    #[cfg(not(feature = "time-driver-tmr0"))]
    run::<_, _, _, 0>(p.TMR0, p.PA9, Irqs0, hal::pac::TMR0, 0).await;
    #[cfg(not(feature = "time-driver-tmr1"))]
    run::<_, _, _, 0>(p.TMR1, p.PA10, Irqs1, hal::pac::TMR1, 1).await;
    #[cfg(not(feature = "time-driver-tmr2"))]
    run::<_, _, _, 0>(p.TMR2, p.PA11, Irqs2, hal::pac::TMR2, 2).await;
    #[cfg(not(feature = "time-driver-tmr3"))]
    run::<_, _, _, 1>(p.TMR3, p.PA2, Irqs3, hal::pac::TMR3, 3).await;
    record(1, 100);
    record(0, 0xc0de5851);
    defmt::info!("timer async characterization complete");
    core::future::pending::<()>().await;
}

#[qingke_rt::entry]
fn main() -> ! {
    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };
    executor.run(|spawner| {
        spawner.spawn(exercise().unwrap());
    })
}
