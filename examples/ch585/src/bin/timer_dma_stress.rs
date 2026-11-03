//! Detached failure-path, repeated rearm and stale-IRQ DMA stress acceptance.
#![no_std]
#![no_main]
use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::interrupt::typelevel::{Binding, Interrupt};
use hal::timer::input_capture::InputCapture;
use hal::timer::simple_pwm::SimplePwm;
use hal::timer::{CaptureEdge, Instance, InterruptHandler, OutputPolarity, PeriodTicks, Repeat, TimerPin};
use hal::Peri;
use panic_halt as _;

#[no_mangle]
pub static mut STRESS_RESULTS: [u32; 64] = [0; 64];
fn record(n: usize, value: u32) {
    unsafe {
        core::ptr::addr_of_mut!(STRESS_RESULTS)
            .cast::<u32>()
            .add(n)
            .write_volatile(value)
    }
}
fn read_result(n: usize) -> u32 {
    unsafe { core::ptr::addr_of!(STRESS_RESULTS).cast::<u32>().add(n).read_volatile() }
}
#[cfg(not(feature = "time-driver-tmr0"))]
hal::bind_interrupts!(struct Irqs0 { TMR0 => InterruptHandler<hal::peripherals::TMR0>; });
#[cfg(not(feature = "time-driver-tmr1"))]
hal::bind_interrupts!(struct Irqs1 { TMR1 => InterruptHandler<hal::peripherals::TMR1>; });
#[cfg(not(feature = "time-driver-tmr2"))]
hal::bind_interrupts!(struct Irqs2 { TMR2 => InterruptHandler<hal::peripherals::TMR2>; });
#[cfg(not(feature = "time-driver-tmr3"))]
hal::bind_interrupts!(struct Irqs3 { TMR3 => InterruptHandler<hal::peripherals::TMR3>; });

static RX: [hal::dma::StaticBuffer<18>; 4] = [const { hal::dma::StaticBuffer::new([0; 18]) }; 4];
static TX: [hal::dma::StaticBuffer<18>; 4] = [const { hal::dma::StaticBuffer::new([0; 18]) }; 4];
static SMALL1: [hal::dma::StaticBuffer<1>; 4] = [const { hal::dma::StaticBuffer::new([0; 1]) }; 4];
static SMALL2: [hal::dma::StaticBuffer<2>; 4] = [const { hal::dma::StaticBuffer::new([0; 2]) }; 4];
static SMALL4: [hal::dma::StaticBuffer<4>; 4] = [const { hal::dma::StaticBuffer::new([0; 4]) }; 4];
static SMALL8: [hal::dma::StaticBuffer<8>; 4] = [const { hal::dma::StaticBuffer::new([0; 8]) }; 4];
static EMPTY_PWM: [hal::dma::StaticBuffer<0>; 4] = [const { hal::dma::StaticBuffer::new([]) }; 4];
static EMPTY_RX: [hal::dma::StaticBuffer<0>; 4] = [const { hal::dma::StaticBuffer::new([]) }; 4];
static EMPTY_RING: [hal::dma::StaticBuffer<0>; 4] = [const { hal::dma::StaticBuffer::new([]) }; 4];
fn metric(id: usize, field: usize, value: u32) {
    record(4 + id * 6 + field, value);
}
fn stopped(r: hal::pac::tmr::TmrFull) {
    assert!(r.ctrl_dma().read().0 == 0 && r.ctrl_mod().read().0 & 0x0c == 0 && r.inter_en().read().0 == 0);
}
async fn capture_twice<T: Instance, I: Binding<T::Interrupt, InterruptHandler<T>> + Copy>(
    cap: &mut InputCapture<'_, T>,
    mut b: &'static mut [u32],
    irq: I,
) -> &'static mut [u32] {
    let length = b.len();
    for _ in 0..2 {
        b.fill(0xdeadbeef);
        let mut tr = cap.read(b).map_err(|_| ()).unwrap();
        assert!(embassy_time::with_timeout(embassy_time::Duration::from_millis(25), tr.wait(irq)).await == Ok(Ok(())));
        assert!(tr.position() == length);
        b = tr.stop();
        assert!(b.iter().all(|v| *v == 0x020130b0));
    }
    b
}
async fn run<T: Instance, P: TimerPin<T, R>, I: Binding<T::Interrupt, InterruptHandler<T>> + Copy, const R: u8>(
    mut peri: Peri<'static, T>,
    mut pin: Peri<'static, P>,
    irq: I,
    regs: hal::pac::tmr::TmrFull,
    id: usize,
) {
    record(1, 10 + id as u32);
    let period = PeriodTicks::new(78_000).unwrap();
    let tx = TX[id].take().unwrap();
    let (tx_a, rest) = tx.split_at_mut(1);
    let (mut tx, tx_b) = rest.split_at_mut(16);
    tx_a[0] = 0x12345678;
    tx_b[0] = 0x87654321;
    let mut pwm = SimplePwm::new::<R>(
        peri.reborrow(),
        pin.reborrow(),
        period,
        OutputPolarity::ActiveHigh,
        Repeat::Once,
    );
    pwm.set_duty(period.get()).unwrap();
    let mode = regs.ctrl_mod().read().0;
    assert!(mode & 0x0c == 0x0c);
    let empty = EMPTY_PWM[id].take().unwrap();
    let (error, empty) = match pwm.write(empty) {
        Err(e) => e,
        Ok(_) => panic!(),
    };
    assert!(error == hal::dma::Error::Empty && empty.is_empty());
    assert!(regs.ctrl_mod().read().0 == mode && regs.ctrl_dma().read().0 == 0);
    tx.fill(period.get() + 1);
    let (error, returned) = match pwm.write(tx) {
        Err(e) => e,
        Ok(_) => panic!(),
    };
    tx = returned;
    assert!(error == hal::dma::Error::InvalidWidth && tx.iter().all(|v| *v == period.get() + 1));
    assert!(pwm.set_duty(period.get() + 1) == Err(hal::timer::Error::InvalidDuty));
    assert!(regs.ctrl_mod().read().0 == mode);
    drop(pwm);
    let mut cap = InputCapture::new::<R>(
        peri.reborrow(),
        pin.reborrow(),
        hal::gpio::Pull::Down,
        period,
        CaptureEdge::RisingToRising,
    );
    let mode = regs.ctrl_mod().read().0;
    let empty = EMPTY_RX[id].take().unwrap();
    let (error, empty) = match cap.read(empty) {
        Err(e) => e,
        Ok(_) => panic!(),
    };
    assert!(error == hal::dma::Error::Empty && empty.is_empty() && regs.ctrl_mod().read().0 == mode);
    let empty = EMPTY_RING[id].take().unwrap();
    let (error, empty) = match cap.read_circular(empty) {
        Err(e) => e,
        Ok(_) => panic!(),
    };
    assert!(error == hal::dma::Error::Empty && empty.is_empty() && regs.ctrl_mod().read().0 == mode);
    metric(id, 4, 5);
    let _ = capture_twice(&mut cap, SMALL1[id].take().unwrap(), irq).await;
    let _ = capture_twice(&mut cap, SMALL2[id].take().unwrap(), irq).await;
    let _ = capture_twice(&mut cap, SMALL4[id].take().unwrap(), irq).await;
    let _ = capture_twice(&mut cap, SMALL8[id].take().unwrap(), irq).await;
    let rx = RX[id].take().unwrap();
    let (rx_a, rest) = rx.split_at_mut(1);
    let (mut rx, rx_b) = rest.split_at_mut(16);
    rx_a[0] = 0x12345678;
    rx_b[0] = 0x87654321;
    rx = capture_twice(&mut cap, rx, irq).await;
    metric(id, 1, 10);
    for n in 0..64 {
        record(1, 1000 + id as u32 * 100 + n);
        rx.fill(0xdeadbeef);
        let mut tr = if n % 2 == 0 {
            cap.read(rx)
        } else {
            cap.read_circular(rx)
        }
        .map_err(|_| ())
        .unwrap();
        let delay = [0, 1, 20, 100, 500, 999, 1500, 4000][n as usize % 8];
        record(29, 1);
        let before = embassy_time::Instant::now();
        let rtc = hal::pac::RTC.cnt_32k().read().cnt_32k();
        let outcome = embassy_time::with_timeout(embassy_time::Duration::from_micros(delay), tr.wait(irq)).await;
        record(29, 2);
        record(
            30,
            match outcome {
                Err(_) => 0,
                Ok(Ok(())) => 1,
                Ok(Err(_)) => 2,
            },
        );
        record(31, tr.position() as u32);
        let slot = 32 + n as usize % 8 * 4;
        let elapsed = before.elapsed().as_micros() as u32;
        let rtc_elapsed = hal::pac::RTC.cnt_32k().read().cnt_32k().wrapping_sub(rtc) as u32;
        record(slot, n);
        record(slot + 1, elapsed);
        record(slot + 2, rtc_elapsed);
        record(slot + 3, tr.position() as u32);
        let excess = elapsed.saturating_sub(delay as u32);
        record(28, read_result(28).max(excess));
        // The timeout must wake independently, not just win on a later DMA IRQ.
        // This is a 1 ms test budget, not an NFC interrupt-deadline guarantee.
        assert!(excess <= 1000 && rtc_elapsed <= ((delay + 1000) * 32768).div_ceil(1_000_000) as u32 + 3);
        assert!(outcome.is_err());
        assert!(tr.position() <= 16);
        rx = tr.stop();
        record(29, 3);
        stopped(regs);
        rx.fill(0xa55aa55a);
        // Deliver an already-pending IRQ after stop, before the next rearm.
        T::Interrupt::pend();
        embassy_time::Timer::after_micros(200).await;
        record(29, 4);
        record(
            31,
            regs.int_flag().read().0 as u32 | ((T::Interrupt::is_pending() as u32) << 31),
        );
        assert!(!T::Interrupt::is_pending());
        record(29, 5);
        assert!(rx.iter().all(|v| *v == 0xa55aa55a));
        record(29, 6);
        assert!(rx_a[0] == 0x12345678 && rx_b[0] == 0x87654321);
        metric(id, 0, n + 1);
        metric(id, 5, n + 1);
    }
    drop(cap);
    for n in 0..16 {
        record(1, 2000 + id as u32 * 100 + n);
        let repeat = [Repeat::Once, Repeat::Four, Repeat::Eight, Repeat::Sixteen][n as usize % 4];
        let mut pwm = SimplePwm::new::<R>(
            peri.reborrow(),
            pin.reborrow(),
            PeriodTicks::new(7800).unwrap(),
            OutputPolarity::ActiveHigh,
            repeat,
        );
        tx.fill(1950);
        tx[15] = 0;
        let tr = pwm.write(tx).map_err(|_| ()).unwrap();
        tx = match embassy_time::with_timeout(embassy_time::Duration::from_millis(60), tr.finish(irq)).await {
            Ok(Ok(b)) => b,
            _ => panic!(),
        };
        stopped(regs);
        assert!(tx.iter().take(15).all(|v| *v == 1950) && tx[15] == 0);
        assert!(tx_a[0] == 0x12345678 && tx_b[0] == 0x87654321);
        T::Interrupt::pend();
        embassy_time::Timer::after_micros(200).await;
        metric(id, 2, n + 1);
    }
    metric(id, 3, 1);
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
    record(3, 1);
    record(1, 100);
    record(0, 0xc0de5854);
    defmt::info!("timer DMA stress characterization complete");
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
