//! Timer/DMA board characterization. No jumpers required: capture is stimulated
//! through its own GPIO pad; PWM is sampled from the pad input. Disconnect loads
//! from PA9/PA10/PA11/PA2 before running. Do not press board buttons during tests.
#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::gpio::Pull;
use hal::timer::input_capture::InputCapture;
use hal::timer::low_level::Timer;
use hal::timer::simple_pwm::SimplePwm;
use hal::timer::{CaptureEdge, Instance, OutputPolarity, PeriodTicks, Repeat, TimerPin};
use hal::Peri;
use panic_halt as _;

static STORAGE: hal::dma::StaticBuffer<66> = hal::dma::StaticBuffer::new([0; 66]);
fn rtc() -> u16 {
    hal::pac::RTC.cnt_32k().read().cnt_32k()
}
fn wait_ticks(n: u16) {
    let s = rtc();
    while rtc().wrapping_sub(s) < n {
        core::hint::spin_loop();
    }
}
fn p(n: u32) -> PeriodTicks {
    PeriodTicks::new(n).unwrap()
}

fn exercise<T: Instance, P: TimerPin<T, R>, const R: u8>(
    mut peri: Peri<'static, T>,
    mut pin: Peri<'static, P>,
    bit: u8,
    regs: hal::pac::tmr::TmrFull,
    mut buffer: &'static mut [u32],
    id: u8,
) -> &'static mut [u32] {
    let mut timer = Timer::new(peri.reborrow());
    timer.configure_counter(p(1_000_000));
    timer.start();
    let a = timer.count();
    let start = rtc();
    while rtc().wrapping_sub(start) < 128 {
        core::hint::spin_loop();
    }
    let b = timer.count();
    defmt::info!(
        "T{} counter rtc128 count {} -> {} flags={=u8:x}",
        id,
        a,
        b,
        timer.events().bits()
    );
    timer.stop();
    let stopped = timer.count();
    wait_ticks(4);
    defmt::info!("T{} stop stable={}", id, timer.count() == stopped);
    drop(timer);

    let mut pwm = SimplePwm::new::<R>(
        peri.reborrow(),
        pin.reborrow(),
        p(78_000),
        OutputPolarity::ActiveHigh,
        Repeat::Once,
    );
    pwm.set_duty(19_500).unwrap();
    let start = rtc();
    let mut high = 0u32;
    let mut low = 0u32;
    let mut rises = 0u32;
    let mut was = false;
    while rtc().wrapping_sub(start) < 1024 {
        let level = hal::pac::GPIO.pa_pin().read().0 & (1 << bit) != 0;
        if level {
            high += 1;
        } else {
            low += 1;
        }
        if level && !was {
            rises += 1;
        }
        was = level;
    }
    defmt::info!(
        "T{} PWM high={} low={} rises={} fifo={} flags={=u8:x}",
        id,
        high,
        low,
        rises,
        regs.fifo_count().read().fifo_count(),
        regs.int_flag().read().0
    );
    pwm.disable();
    for (i, v) in buffer.iter_mut().enumerate() {
        *v = if i % 2 == 0 { 19_500 } else { 58_500 };
    }
    let transfer = pwm.write(buffer).map_err(|_| ()).unwrap();
    let start = rtc();
    while !transfer.is_complete() && rtc().wrapping_sub(start) < 4096 {
        core::hint::spin_loop();
    }
    let before = regs.int_flag().read().0;
    regs.inter_en().write(|w| w.0 = 8);
    let after = regs.int_flag().read().0;
    regs.inter_en().write(|w| w.0 = 0);
    defmt::info!(
        "T{} DMA flag IE probe before={=u8:x} enabled={=u8:x}",
        id,
        before,
        after
    );
    defmt::info!(
        "T{} TX DMA done={} pos={} flags={=u8:x} fifo={}",
        id,
        transfer.is_complete(),
        transfer.position(),
        regs.int_flag().read().0,
        regs.fifo_count().read().fifo_count()
    );
    // bounded polling before calling the blocking drain operation
    if transfer.is_complete() {
        buffer = transfer.blocking_finish().map_err(|_| ()).unwrap();
    } else {
        buffer = transfer.stop();
    }
    defmt::info!(
        "T{} TX stopped ctrl={=u8:x} dma={=u8:x}",
        id,
        regs.ctrl_mod().read().0,
        regs.ctrl_dma().read().0
    );
    drop(pwm);

    buffer.fill(0xdead_beef);
    let mut cap = InputCapture::new::<R>(
        peri.reborrow(),
        pin.reborrow(),
        Pull::Down,
        p(78_000),
        CaptureEdge::RisingToRising,
    );
    let transfer = cap.read(buffer).map_err(|_| ()).unwrap();
    // Deliberate test-only ownership bypass: the capture driver owns this pad.
    // Driving the same pad stimulates the input path without an external wire.
    hal::pac::GPIO.pa_clr().write(|w| w.0 = 1 << bit);
    hal::pac::GPIO.pa_dir().modify(|w| w.0 |= 1 << bit);
    for _ in 0..80 {
        hal::pac::GPIO.pa_set().write(|w| w.0 = 1 << bit);
        wait_ticks(4);
        hal::pac::GPIO.pa_clr().write(|w| w.0 = 1 << bit);
        wait_ticks(4);
        if transfer.is_complete() {
            break;
        }
    }
    let done = transfer.is_complete();
    let pos = transfer.position();
    buffer = transfer.stop();
    hal::pac::GPIO.pa_dir().modify(|w| w.0 &= !(1 << bit));
    defmt::info!(
        "T{} RX done={} pos={} first={=u32:x},{=u32:x},{=u32:x},{=u32:x}",
        id,
        done,
        pos,
        buffer[0],
        buffer[1],
        buffer[2],
        buffer[3]
    );
    drop(cap);
    buffer
}

#[qingke_rt::entry]
fn main() -> ! {
    let p = hal::init(hal::rcc::Config::HSE_PLL_78MHZ.into());
    defmt::info!("timer DMA board characterization start");
    let storage = STORAGE.take().unwrap();
    storage[0] = 0x12345678;
    storage[65] = 0x87654321;
    let (guard_a, rest) = storage.split_at_mut(1);
    let (mut buffer, guard_b) = rest.split_at_mut(64);
    #[cfg(not(feature = "time-driver-tmr0"))]
    {
        buffer = exercise::<_, _, 0>(p.TMR0, p.PA9, 9, hal::pac::TMR0, buffer, 0);
    }
    #[cfg(not(feature = "time-driver-tmr1"))]
    {
        buffer = exercise::<_, _, 0>(p.TMR1, p.PA10, 10, hal::pac::TMR1, buffer, 1);
    }
    #[cfg(not(feature = "time-driver-tmr2"))]
    {
        buffer = exercise::<_, _, 0>(p.TMR2, p.PA11, 11, hal::pac::TMR2, buffer, 2);
    }
    #[cfg(not(feature = "time-driver-tmr3"))]
    {
        buffer = exercise::<_, _, 1>(p.TMR3, p.PA2, 2, hal::pac::TMR3, buffer, 3);
    }
    let _ = buffer;
    defmt::info!(
        "timer DMA characterization complete guards={=u32:x}/{=u32:x}",
        guard_a[0],
        guard_b[0]
    );
    loop {
        core::hint::spin_loop();
    }
}
