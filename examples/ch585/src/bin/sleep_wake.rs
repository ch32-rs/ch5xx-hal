//! Halt and sleep, waking on the RTC.
//!
//! Nothing is wired. The RTC trigger is armed one second ahead, the core is
//! stopped with [`power::halt`], and the 32 kHz count read on both sides of the
//! call proves the chip slept and the RTC kept running. The same is then done
//! with [`power::sleep`], which turns the main LDO off and retains both SRAM
//! blocks.
//!
//! The clock tree is HSE PLL 78 MHz, so every wake also proves that the PMU
//! restarted the crystal and the PLL before the core resumed. A blocking delay
//! measured against the RTC at the end checks that the tree is still at the
//! configured rate.
//!
//! The debug probe cannot reach the core while it is stopped in a low-power
//! mode, so RTT may drop out during the sleeps; re-attach with
//! `probe-rs attach` to read the result line.
//!
//! Build without the Embassy time driver, so SysTick belongs to the blocking
//! `Delay`:
//!
//! ```console
//! cargo run --release --bin sleep_wake --no-default-features
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::power::{self, Retention, WakeDelay, WakeSources};
use defmt_rtt as _;
use hal::delay::Delay;
use hal::rtc::Rtc;
use panic_halt as _;
use qingke_rt::entry;

/// One second of 32 kHz counts.
const ONE_SECOND: u32 = 32_768;
/// Accepted error around the expected interval. The alarm, the wake path, the
/// restore code and the counter read all fall inside this window.
const TOLERANCE: u32 = 2_000;
/// Delay used to verify the clock tree after the wakes, and its expected RTC
/// counts (200 ms at 32.768 kHz).
const PROBE_MS: u32 = 200;
const PROBE_COUNTS: u32 = ONE_SECOND * PROBE_MS / 1000;

/// Arm the alarm one second ahead and sleep; returns the 32 kHz interval it
/// measured.
macro_rules! sleep_until_rtc {
    ($rtc:expr, $sleep:expr) => {{
        $rtc.clear_trigger();
        let start = $rtc.cycle_count();
        $rtc.set_alarm_in(ONE_SECOND);
        $sleep;
        let slept = $rtc.cycle_count().wrapping_sub(start);
        if !$rtc.trigger_pending() {
            defmt::error!("woke without the RTC trigger");
        }
        $rtc.clear_trigger();
        slept
    }};
}

#[entry]
fn main() -> ! {
    let p = hal::init(hal::rcc::Config::HSE_PLL_78MHZ.into());
    let mut rtc = Rtc::new(p.RTC);
    let mut delay = Delay;

    // The RTC keeps its configuration across a system reset, so a previous
    // program may have left the interval timer running and its flag set. A
    // pending RTC flag is also a wake event, which would end every sleep here
    // at once.
    rtc.stop_timer();
    rtc.clear_timer();
    rtc.clear_trigger();

    defmt::info!(
        "sleep_wake: sysclk {} Hz, three halt wakes",
        hal::rcc::clocks().sysclk.0
    );

    let mut halt_ok = 0;
    for _ in 0..3 {
        let slept = sleep_until_rtc!(rtc, power::halt(WakeSources::RTC, WakeDelay::Long));
        halt_ok += in_window(slept) as u32;
        defmt::info!("halt: slept {} cycles", slept);
    }

    let mut sleep_ok = 0;
    for _ in 0..3 {
        let slept = sleep_until_rtc!(
            rtc,
            power::sleep(
                Retention::RAM96K | Retention::RAM32K,
                WakeSources::RTC,
                WakeDelay::Long
            )
        );
        sleep_ok += in_window(slept) as u32;
        defmt::info!("sleep: slept {} cycles", slept);
    }

    // A missed PLL or crystal restart would show up here as a delay that is
    // wrong against the independent 32 kHz reference.
    let start = rtc.cycle_count();
    delay.delay_ms(PROBE_MS);
    let probe = rtc.cycle_count().wrapping_sub(start);
    let clock_ok = probe.abs_diff(PROBE_COUNTS) < PROBE_COUNTS / 10;

    if halt_ok == 3 && sleep_ok == 3 && clock_ok {
        defmt::info!(
            "PASS sleep_wake: halt 3/3 sleep 3/3, {} ms delay = {} cycles",
            PROBE_MS,
            probe
        );
    } else {
        defmt::error!(
            "FAIL sleep_wake: halt {}/3 sleep {}/3, {} ms delay = {} cycles",
            halt_ok,
            sleep_ok,
            PROBE_MS,
            probe
        );
    }

    loop {
        core::hint::spin_loop();
    }
}

fn in_window(counts: u32) -> bool {
    (ONE_SECOND - TOLERANCE..=ONE_SECOND + TOLERANCE).contains(&counts)
}
