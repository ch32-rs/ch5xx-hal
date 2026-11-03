//! Independent-clock capture qualification using a 3.3 V USB-UART TX source.
//! Connect TX to PA9/PA10/PA11/PA2 and share GND; disconnect other loads.
//! This firmware only uses those pads as inputs. Never connect a 5 V TTL source.
#![no_std]
#![no_main]
use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::timer::input_capture::InputCapture;
use hal::timer::{CaptureEdge, Instance, PeriodTicks, TimerPin};
use hal::Peri;
use panic_halt as _;

#[no_mangle]
pub static mut EXTERNAL_RESULTS: [u32; 80] = [0; 80];
static BUFFER: hal::dma::StaticBuffer<258> = hal::dma::StaticBuffer::new([0; 258]);
fn put(n: usize, value: u32) {
    unsafe {
        core::ptr::addr_of_mut!(EXTERNAL_RESULTS)
            .cast::<u32>()
            .add(n)
            .write_volatile(value)
    }
}
fn rtc() -> u16 {
    hal::pac::RTC.cnt_32k().read().cnt_32k()
}
fn exercise<T: Instance, P: TimerPin<T, R>, const R: u8>(
    mut t: Peri<'_, T>,
    mut pin: Peri<'_, P>,
    mut buffer: &'static mut [u32],
    id: usize,
) -> &'static mut [u32] {
    for (mode, edge) in [
        CaptureEdge::Both,
        CaptureEdge::FallingToFalling,
        CaptureEdge::RisingToRising,
    ]
    .into_iter()
    .enumerate()
    {
        put(1, 10 + id as u32 * 3 + mode as u32);
        let mut capture = InputCapture::new::<R>(
            t.reborrow(),
            pin.reborrow(),
            hal::gpio::Pull::Down,
            PeriodTicks::new_capture(780_000).unwrap(),
            edge,
        );
        buffer.fill(0xdeadbeef);
        let transfer = capture.read(buffer).map_err(|_| ()).unwrap();
        let start = rtc();
        while !transfer.is_complete() && rtc().wrapping_sub(start) < 32768 {
            core::hint::spin_loop();
        }
        assert!(transfer.is_complete() && transfer.position() == 256);
        assert!(transfer.error().is_none());
        buffer = transfer.stop();
        let expected = if mode == 0 { 8125 } else { 16250 };
        let mut count = 0;
        let mut min = u32::MAX;
        let mut max = 0;
        let mut metadata_errors = 0;
        let mut previous = None;
        // Ignore initial partial intervals and long idle timeout fragments.
        for raw in buffer.iter().skip(4) {
            let word = hal::timer::CaptureWord(*raw);
            let ticks = word.ticks();
            if ticks < expected * 97 / 100 || ticks > expected * 103 / 100 {
                previous = None;
                continue;
            }
            count += 1;
            min = min.min(ticks);
            max = max.max(ticks);
            if mode == 0 {
                let level = word.high_level();
                if previous == Some(level) {
                    metadata_errors += 1;
                }
                previous = Some(level);
            } else if word.timeout_fragment() {
                metadata_errors += 1;
            }
        }
        let base = 8 + id * 16 + mode * 5;
        put(base, count);
        put(base + 1, min);
        put(base + 2, max);
        put(base + 3, metadata_errors);
        put(base + 4, 1);
        let first_active = if cfg!(feature = "time-driver-tmr0") { 1 } else { 0 };
        let minimum = if id == first_active && mode == 0 { 128 } else { 252 };
        assert!(count >= minimum && metadata_errors == 0);
    }
    buffer
}
#[qingke_rt::entry]
fn main() -> ! {
    let p = hal::init(hal::rcc::Config::HSE_PLL_78MHZ.into());
    put(2, hal::rcc::clocks().sysclk.0);
    put(3, 9600);
    put(4, 1);
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
    let storage = BUFFER.take().unwrap();
    storage[0] = 0x12345678;
    storage[257] = 0x87654321;
    let (guard_a, rest) = storage.split_at_mut(1);
    let (mut buffer, guard_b) = rest.split_at_mut(256);
    #[cfg(not(feature = "time-driver-tmr0"))]
    {
        buffer = exercise::<_, _, 0>(p.TMR0, p.PA9, buffer, 0);
    }
    #[cfg(not(feature = "time-driver-tmr1"))]
    {
        buffer = exercise::<_, _, 0>(p.TMR1, p.PA10, buffer, 1);
    }
    #[cfg(not(feature = "time-driver-tmr2"))]
    {
        buffer = exercise::<_, _, 0>(p.TMR2, p.PA11, buffer, 2);
    }
    #[cfg(not(feature = "time-driver-tmr3"))]
    {
        buffer = exercise::<_, _, 1>(p.TMR3, p.PA2, buffer, 3);
    }
    let _ = buffer;
    put(6, guard_a[0]);
    put(7, guard_b[0]);
    assert!(guard_a[0] == 0x12345678 && guard_b[0] == 0x87654321);
    put(1, 100);
    put(0, 0xc0de5855);
    loop {
        core::hint::spin_loop();
    }
}
