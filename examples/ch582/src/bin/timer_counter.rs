//! Timer counters on CH582, covering both TMR register blocks.
//!
//! TMR0 and TMR3 are `TMR_NODMA` on this die: they have the counter, FIFO and
//! interrupt registers but no timer-local DMA. The driver now compiles for
//! both block types, and this example checks that the counter API behaves the
//! same on a no-DMA instance (TMR0) and a full one (TMR2).
//!
//! No external signal pins are used. Each timer counts at `Fsys` while the RTC
//! measures the same interval. The comparison assumes a 32,768 Hz RTC clock;
//! oscillator error affects the result. A 1 kHz period must raise `CYCLE_END`.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::timer::low_level::Timer;
use ch5xx_hal::timer::{Events, PeriodTicks, MAX_PERIOD};
use defmt::{error, info};
use defmt_rtt as _;
use hal::delay::Delay;
use hal::rtc::Rtc;
use panic_halt as _;
use qingke_rt::entry;

const MEASURE_MS: u32 = 100;
/// Accepted error between the Fsys tick count and the RTC-measured interval.
const TOLERANCE_PCT: u64 = 5;

#[entry]
fn main() -> ! {
    let p = hal::init(hal::rcc::Config::default().into());
    let mut rtc = Rtc::new(p.RTC);
    let mut delay = Delay;
    rtc.stop_timer();
    rtc.clear_timer();
    rtc.clear_trigger();

    let sysclk = hal::rcc::clocks().sysclk.0;
    info!("timer counter: Fsys {} Hz", sysclk);

    let mut ok = 0;
    ok += check_counter(&rtc, &mut delay, p.TMR0, "TMR0 (nodma)", sysclk);
    ok += check_counter(&rtc, &mut delay, p.TMR2, "TMR2 (full)", sysclk);

    if ok == 2 {
        info!("SELFTEST PASS: 2/2 counters");
    } else {
        error!("SELFTEST FAIL: {}/2 counters", ok);
    }

    loop {
        core::hint::spin_loop();
    }
}

/// Count for `MEASURE_MS` and compare the tick rate with the RTC interval.
fn check_counter<T: hal::timer::Instance>(
    rtc: &Rtc<'_, hal::peripherals::RTC>,
    delay: &mut Delay,
    peri: hal::Peri<'_, T>,
    name: &str,
    sysclk: u32,
) -> u32 {
    let period = PeriodTicks::new(MAX_PERIOD).unwrap();
    // 1 kHz at the current Fsys, to exercise the cycle-end event afterwards.
    let khz = PeriodTicks::from_hz(sysclk, 1000).unwrap();
    let mut timer = Timer::new(peri);

    timer.configure_counter(period);
    timer.start();
    let start_rtc = rtc.cycle_count();
    let start = timer.count();
    delay.delay_ms(MEASURE_MS);
    let ticks = timer.count().wrapping_sub(start);
    let end_rtc = rtc.cycle_count();
    // The RTC cycle count resets at midnight rather than at u32::MAX.
    let rtc_ticks = if end_rtc >= start_rtc {
        end_rtc - start_rtc
    } else {
        (0xA8C0_0000 - start_rtc) + end_rtc
    };
    let expected = (rtc_ticks as u64 * sysclk as u64 / 32_768) as u32;
    let rate_ok = expected > 0 && ticks.abs_diff(expected) as u64 * 100 <= expected as u64 * TOLERANCE_PCT;

    timer.configure_counter(khz);
    timer.start();
    delay.delay_ms(10);
    let cycle_ok = timer.events().contains(Events::CYCLE_END);
    timer.acknowledge(Events::ALL);

    if rate_ok && cycle_ok {
        info!(
            "{}: {} ticks in {} RTC ticks (expected {}), cycle end",
            name, ticks, rtc_ticks, expected
        );
        1
    } else {
        error!(
            "{}: {} ticks, expected {}, cycle end {}",
            name,
            ticks,
            expected,
            if cycle_ok { "raised" } else { "missing" }
        );
        0
    }
}
