//! Silent SRAM-recorded board acceptance. No RTT reads during measurement.
//! External loads must be disconnected from PA9, PA10, PA11 and PA2.
#![no_std]
#![no_main]
use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::timer::input_capture::InputCapture;
use hal::timer::low_level::Timer;
use hal::timer::simple_pwm::SimplePwm;
use hal::timer::{CaptureEdge, Instance, NfcClockInstance, OutputPolarity, PeriodTicks, Repeat, TimerPin};
use hal::Peri;
use panic_halt as _;

#[no_mangle]
pub static mut TIMER_RESULTS: [u32; 384] = [0; 384];
static BUFFER: hal::dma::StaticBuffer<18> = hal::dma::StaticBuffer::new([0; 18]);
#[link_section = ".dma_probe_a"]
static mut PROBE_A: [u32; 18] = [0; 18];
#[link_section = ".dma_probe_b"]
static mut PROBE_B: [u32; 18] = [0; 18];
#[link_section = ".dma_probe_c"]
static mut PROBE_C: [u32; 18] = [0; 18];
#[link_section = ".dma_probe_d"]
static mut PROBE_D: [u32; 18] = [0; 18];
fn put(n: usize, v: u32) {
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(TIMER_RESULTS).cast::<u32>().add(n), v) }
}
fn record(id: usize, n: usize, v: u32) {
    put(8 + id * 80 + n, v);
}
fn rtc() -> u16 {
    hal::pac::RTC.cnt_32k().read().cnt_32k()
}
fn wait(n: u16) {
    let t = rtc();
    while rtc().wrapping_sub(t) < n {
        core::hint::spin_loop();
    }
}
fn now() -> u64 {
    embassy_time::Instant::now().as_micros()
}
fn period(n: u32) -> PeriodTicks {
    PeriodTicks::new(n).unwrap()
}
fn drive(bit: u8, high: bool) {
    if high {
        hal::pac::GPIO.pa_set().write(|w| w.0 = 1 << bit);
    } else {
        hal::pac::GPIO.pa_clr().write(|w| w.0 = 1 << bit);
    }
}
fn output(bit: u8) {
    drive(bit, false);
    hal::pac::GPIO.pa_dir().modify(|w| w.0 |= 1 << bit);
}
fn input(bit: u8) {
    hal::pac::GPIO.pa_dir().modify(|w| w.0 &= !(1 << bit));
}

fn exercise<'d, T: Instance, P: TimerPin<T, R>, const R: u8>(
    mut t: Peri<'d, T>,
    mut pin: Peri<'d, P>,
    bit: u8,
    r: hal::pac::tmr::TmrFull,
    mut b: &'static mut [u32],
    id: usize,
) -> &'static mut [u32] {
    put(1, 10 + id as u32);
    let mut timer = Timer::new(t.reborrow());
    timer.configure_counter(period(1_000_000));
    timer.start();
    let a = timer.count();
    let start = rtc();
    while rtc().wrapping_sub(start) < 128 {
        core::hint::spin_loop();
    }
    record(id, 0, timer.count().wrapping_sub(a));
    timer.stop();
    let c = timer.count();
    wait(8);
    record(id, 1, (timer.count() == c) as u32);
    drop(timer);
    let mut pwm = SimplePwm::new::<R>(
        t.reborrow(),
        pin.reborrow(),
        period(78_000),
        OutputPolarity::ActiveHigh,
        Repeat::Once,
    );
    pwm.set_duty(19_500).unwrap();
    let start = rtc();
    let mut prev = false;
    let mut last = now();
    let mut highs = 0u32;
    let mut lows = 0u32;
    let mut rises = 0u32;
    while rtc().wrapping_sub(start) < 1024 {
        let level = hal::pac::GPIO.pa_pin().read().0 & (1 << bit) != 0;
        if level != prev {
            let n = now();
            if rises > 1 {
                if prev {
                    highs += (n - last) as u32;
                } else {
                    lows += (n - last) as u32;
                }
            }
            if level {
                rises += 1;
            }
            prev = level;
            last = n;
        }
    }
    record(id, 2, rises);
    record(id, 3, highs);
    record(id, 4, lows);
    pwm.set_duty(0).unwrap();
    wait(64);
    record(id, 5, (hal::pac::GPIO.pa_pin().read().0 >> bit) & 1);
    pwm.set_duty(78_000).unwrap();
    wait(64);
    record(id, 6, (hal::pac::GPIO.pa_pin().read().0 >> bit) & 1);
    b.fill(19_500);
    let tr = pwm.write(b).map_err(|_| ()).unwrap();
    let start = rtc();
    while !tr.is_complete() && rtc().wrapping_sub(start) < 4096 {
        core::hint::spin_loop();
    }
    record(id, 7, tr.is_complete() as u32);
    record(id, 8, tr.position() as u32);
    record(id, 9, r.int_flag().read().0 as u32);
    let before = now();
    b = if tr.is_complete() {
        tr.blocking_finish().map_err(|_| ()).unwrap()
    } else {
        tr.stop()
    };
    record(id, 10, (now() - before) as u32);
    record(id, 11, r.ctrl_dma().read().0 as u32);
    drop(pwm);
    // Each capture mode retains bit 25 rather than silently treating it as time.
    for (i, edge) in [
        CaptureEdge::Both,
        CaptureEdge::FallingToFalling,
        CaptureEdge::RisingToRising,
    ]
    .into_iter()
    .enumerate()
    {
        b.fill(0xdeadbeef);
        let mut cap = InputCapture::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            hal::gpio::Pull::Down,
            period(78_000),
            edge,
        );
        let tr = cap.read(b).map_err(|_| ()).unwrap();
        output(bit);
        for _ in 0..40 {
            drive(bit, true);
            wait(4);
            drive(bit, false);
            wait(4);
            if tr.is_complete() {
                break;
            }
        }
        record(id, 12 + i * 4, tr.is_complete() as u32);
        record(id, 13 + i * 4, tr.position() as u32);
        b = tr.stop();
        record(id, 14 + i * 4, b[2]);
        record(id, 15 + i * 4, b[3]);
        b.fill(0xa55aa55a);
        for _ in 0..4 {
            drive(bit, true);
            wait(4);
            drive(bit, false);
            wait(4);
        }
        record(id, 24 + i, (b.iter().all(|v| *v == 0xa55aa55a)) as u32);
        input(bit);
        drop(cap);
    }
    for (n, edge) in [
        CaptureEdge::Both,
        CaptureEdge::RisingToRising,
        CaptureEdge::FallingToFalling,
    ]
    .into_iter()
    .enumerate()
    {
        let counter = hal::timer::counter::ExternalCounter::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            hal::gpio::Pull::Down,
            period(1_000_000),
            edge,
        );
        output(bit);
        let before = counter.count();
        for _ in 0..20 {
            drive(bit, true);
            wait(2);
            drive(bit, false);
            wait(2);
        }
        record(id, 49 + n, counter.count().wrapping_sub(before));
        input(bit);
        drop(counter);
    }
    {
        let mut cap = InputCapture::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            hal::gpio::Pull::Down,
            period(78_000),
            CaptureEdge::RisingToRising,
        );
        let tr = cap.read_circular(b).map_err(|_| ()).unwrap();
        wait(2048);
        record(id, 52, tr.is_complete() as u32);
        record(id, 53, tr.position() as u32);
        b = tr.stop();
        record(id, 54, b.iter().all(|v| *v == 0x020130b0) as u32);
        b.fill(0xa55aa55a);
        wait(128);
        record(id, 55, b.iter().all(|v| *v == 0xa55aa55a) as u32);
        drop(cap);
    }
    for (n, repeat) in [Repeat::Once, Repeat::Four, Repeat::Eight, Repeat::Sixteen]
        .into_iter()
        .enumerate()
    {
        for (i, v) in b.iter_mut().enumerate() {
            *v = if i % 2 == 1 && i < 15 { 7800 } else { 0 };
        }
        let mut pwm = SimplePwm::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            period(7800),
            OutputPolarity::ActiveHigh,
            repeat,
        );
        let tr = pwm.write(b).map_err(|_| ()).unwrap();
        let start = rtc();
        let mut prev = false;
        let mut rises = 0;
        let mut last = now();
        let mut high_time = 0;
        while rtc().wrapping_sub(start) < 1024 {
            let level = hal::pac::GPIO.pa_pin().read().0 & (1 << bit) != 0;
            if level != prev {
                let t = now();
                if prev {
                    high_time += (t - last) as u32;
                } else {
                    rises += 1;
                }
                last = t;
                prev = level;
            }
        }
        record(id, 56 + n, rises);
        record(id, 60 + n, high_time);
        b = tr.blocking_finish().map_err(|_| ()).unwrap();
        drop(pwm);
    }
    {
        let mut pwm = SimplePwm::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            period(7800),
            OutputPolarity::ActiveLow,
            Repeat::Once,
        );
        pwm.set_duty(0).unwrap();
        wait(64);
        record(id, 64, (hal::pac::GPIO.pa_pin().read().0 >> bit) & 1);
        pwm.set_duty(7800).unwrap();
        wait(64);
        record(id, 65, (hal::pac::GPIO.pa_pin().read().0 >> bit) & 1);
        drop(pwm);
    }
    {
        let mut timer = Timer::new(t.reborrow());
        let mut mask = 0;
        for (n, ticks) in [1, 2, 18, 288].into_iter().enumerate() {
            timer.configure_counter(period(ticks.max(2)));
            // Probe the unsupported END=1 directly, outside the safe API.
            r.cnt_end().write(|w| w.set_cnt_end(ticks));
            timer.start();
            wait(4);
            record(id, 68 + n * 2, timer.events().bits() as u32);
            record(id, 69 + n * 2, timer.count());
            if timer.events().contains(hal::timer::Events::CYCLE_END) && timer.count() <= ticks {
                mask |= 1 << n;
            }
        }
        record(id, 66, mask);
        timer.configure_counter(period(0x03ffffff));
        timer.start();
        wait(128);
        record(id, 67, timer.count());
        drop(timer);
    }
    {
        let mut cap = InputCapture::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            hal::gpio::Pull::Down,
            period(7800),
            CaptureEdge::RisingToRising,
        );
        wait(128);
        record(
            id,
            76,
            (cap.try_capture() == Err(hal::timer::Error::FifoOverrun)) as u32,
        );
        record(id, 77, r.fifo_count().read().fifo_count() as u32);
        drop(cap);
        let mut pwm = SimplePwm::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            period(7800),
            OutputPolarity::ActiveHigh,
            Repeat::Once,
        );
        pwm.set_duty(3900).unwrap();
        wait(128);
        record(id, 78, r.int_flag().read().if_fifo_ov() as u32);
        record(id, 79, r.fifo_count().read().fifo_count() as u32);
        drop(pwm);
    }
    // Verify every possible upper-address truncation lands in reserved storage.
    for index in 0..4 {
        let probes = [
            core::ptr::addr_of_mut!(PROBE_A).cast::<u32>(),
            core::ptr::addr_of_mut!(PROBE_B).cast::<u32>(),
            core::ptr::addr_of_mut!(PROBE_C).cast::<u32>(),
            core::ptr::addr_of_mut!(PROBE_D).cast::<u32>(),
        ];
        for ptr in probes {
            for n in 0..18 {
                unsafe { ptr.add(n).write_volatile(0xcafe0000 + n as u32) }
            }
        }
        let mut timer = Timer::new(t.reborrow());
        unsafe {
            timer.configure_raw(0xc1, period(78_000));
        }
        let begin = (probes[index] as u32 + 4) & 0x1ffff;
        let end = begin + 64;
        r.dma_beg().write(|w| w.0 = begin);
        r.dma_end().write(|w| w.0 = end);
        r.ctrl_dma().write(|w| w.0 = 1);
        timer.start();
        output(bit);
        for _ in 0..20 {
            drive(bit, true);
            wait(4);
            drive(bit, false);
            wait(4);
        }
        timer.stop();
        input(bit);
        record(id, 28 + index * 4, r.dma_beg().read().0);
        record(id, 29 + index * 4, r.dma_now().read().0);
        let mut changed = 0;
        let mut guards = true;
        for (which, ptr) in probes.into_iter().enumerate() {
            let count = (1..17)
                .filter(|n| unsafe { ptr.add(*n).read_volatile() != 0xcafe0000 + *n as u32 })
                .count();
            if count == 16 {
                changed |= 1 << which;
            }
            guards &= unsafe { ptr.read_volatile() == 0xcafe0000 && ptr.add(17).read_volatile() == 0xcafe0011 };
        }
        record(id, 30 + index * 4, changed);
        record(id, 31 + index * 4, guards as u32);
        drop(timer);
        let src = unsafe { core::slice::from_raw_parts_mut(probes[index].add(1), 16) };
        for (n, v) in src.iter_mut().enumerate() {
            *v = if n % 2 == 1 && n < 15 { 7800 } else { 0 };
        }
        let mut pwm = SimplePwm::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            period(7800),
            OutputPolarity::ActiveHigh,
            Repeat::Once,
        );
        let tr = pwm.write(src).map_err(|_| ()).unwrap();
        let start = rtc();
        let mut rises = 0;
        let mut prev = false;
        while rtc().wrapping_sub(start) < 128 {
            let level = hal::pac::GPIO.pa_pin().read().0 & (1 << bit) != 0;
            if level && !prev {
                rises += 1;
            }
            prev = level;
        }
        let done = tr.is_complete();
        let pos = tr.position();
        let _src = tr.stop();
        let guards = unsafe {
            probes[index].read_volatile() == 0xcafe0000 && probes[index].add(17).read_volatile() == 0xcafe0011
        };
        record(
            id,
            44 + index,
            done as u32 | (((pos == 16) as u32) << 1) | ((guards as u32) << 2) | (rises << 8),
        );
        drop(pwm);
    }
    b
}
fn clock<T: NfcClockInstance>(mut t: Peri<'_, T>, id: usize) {
    let mut timer = Timer::new(t.reborrow());
    timer.configure_counter(period(1_000_000));
    timer.set_nfc_clock().unwrap();
    timer.start();
    let a = timer.count();
    let start = rtc();
    while rtc().wrapping_sub(start) < 128 {
        core::hint::spin_loop();
    }
    record(id, 48, timer.count() - a);
}
#[qingke_rt::entry]
fn main() -> ! {
    let mut p = hal::init(hal::rcc::Config::HSE_PLL_78MHZ.into());
    put(0, 0);
    put(6, hal::rcc::clocks().sysclk.0);
    put(7, 2);
    #[cfg(feature = "time-driver-systick")]
    put(5, 0);
    #[cfg(feature = "time-driver-tmr0")]
    put(5, 1);
    #[cfg(feature = "time-driver-tmr1")]
    put(5, 2);
    #[cfg(feature = "time-driver-tmr2")]
    put(5, 3);
    #[cfg(feature = "time-driver-tmr3")]
    put(5, 4);
    put(2, hal::pac::SYS.ck32k_config().read().0 as u32);
    let storage = BUFFER.take().unwrap();
    storage[0] = 0x12345678;
    storage[17] = 0x87654321;
    let (guard_a, rest) = storage.split_at_mut(1);
    let (mut b, guard_b) = rest.split_at_mut(16);
    #[cfg(not(feature = "time-driver-tmr0"))]
    {
        b = exercise::<_, _, 0>(p.TMR0.reborrow(), p.PA9, 9, hal::pac::TMR0, b, 0);
        clock(p.TMR0.reborrow(), 0);
    }
    #[cfg(not(feature = "time-driver-tmr1"))]
    {
        b = exercise::<_, _, 0>(p.TMR1.reborrow(), p.PA10, 10, hal::pac::TMR1, b, 1);
    }
    #[cfg(not(feature = "time-driver-tmr2"))]
    {
        b = exercise::<_, _, 0>(p.TMR2.reborrow(), p.PA11, 11, hal::pac::TMR2, b, 2);
    }
    #[cfg(not(feature = "time-driver-tmr3"))]
    {
        b = exercise::<_, _, 1>(p.TMR3.reborrow(), p.PA2, 2, hal::pac::TMR3, b, 3);
        clock(p.TMR3.reborrow(), 3);
    }
    let _ = b;
    put(3, guard_a[0]);
    put(4, guard_b[0]);
    put(1, 100);
    put(0, 0xc0de5850);
    loop {
        core::hint::spin_loop();
    }
}
