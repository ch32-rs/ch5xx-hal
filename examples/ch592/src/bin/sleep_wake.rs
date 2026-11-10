//! Halt and sleep on CH592X, waking on the RTC.
//!
//! Nothing is wired. The RTC trigger is armed one second ahead, the core is
//! stopped with [`power::halt`], and the 32 kHz count read on both sides of the
//! call proves the chip slept and the RTC kept running. The same is then done
//! with [`power::sleep`], which turns the main LDO off and retains the 2 KB and
//! 24 KB SRAM blocks.
//!
//! The clock tree is the 480 MHz PLL divided to 48 MHz, so every wake also
//! proves that the PMU restarted the crystal and the PLL before the core
//! resumed. A blocking delay measured against the RTC at the end checks that the
//! tree is still at the configured rate and that SysTick survived the wake
//! reset.
//!
//! The debug probe cannot reach the core while it is stopped in a low-power
//! mode, so the RTT session ends at the first sleep. The verdict is therefore
//! recorded in the `RESULT` array, whose address is printed before the first
//! sleep; read it back with `probe-rs read` once the run has finished:
//!
//! ```console
//! cargo run --release --bin sleep_wake --no-default-features
//! probe-rs read --chip CH592 b32 <address from the log> 8
//! ```
//!
//! `RESULT[0]` is `0x5ee9_5ee9` once the run finished, `RESULT[1]` counts the
//! wakes inside the expected window (out of 6), `RESULT[2]` is the RTC count of
//! the 200 ms blocking delay, `RESULT[3..6]` the three halt intervals and
//! `RESULT[6..9]` the three sleep intervals.

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
/// Written to `RESULT[0]` when the last measurement is stored.
const MAGIC: u32 = 0x5ee9_5ee9;

/// Measurements, kept in SRAM so they survive the sleeps and can be read back
/// over the debug probe afterwards.
#[no_mangle]
pub static mut RESULT: [u32; 9] = [0; 9];

fn store(index: usize, value: u32) {
    // SAFETY: this example is single-threaded and `RESULT` is not borrowed
    // elsewhere.
    unsafe { core::ptr::write_volatile(&raw mut RESULT[index], value) };
}

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
    let p = hal::init(hal::rcc::Config::PLL_48MHZ.into());
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
        "sleep_wake: sysclk {} Hz, result at {:x}, three halt wakes",
        hal::rcc::clocks().sysclk.0,
        &raw const RESULT as usize
    );

    let mut wake_ok = 0;
    for i in 0..3 {
        let slept = sleep_until_rtc!(rtc, power::halt(WakeSources::RTC, WakeDelay::Long));
        wake_ok += in_window(slept) as u32;
        store(3 + i, slept);
        defmt::info!("halt: slept {} cycles", slept);
    }

    for i in 0..3 {
        let slept = sleep_until_rtc!(
            rtc,
            power::sleep(
                Retention::RAM24K | Retention::RAM2K,
                WakeSources::RTC,
                WakeDelay::Long
            )
        );
        wake_ok += in_window(slept) as u32;
        store(6 + i, slept);
        defmt::info!("sleep: slept {} cycles", slept);
    }

    // A missed PLL or crystal restart would show up here as a delay that is
    // wrong against the independent 32 kHz reference. A SysTick that the wake
    // reset cleared would instead spin forever, so the run never terminates.
    let start = rtc.cycle_count();
    delay.delay_ms(PROBE_MS);
    let probe = rtc.cycle_count().wrapping_sub(start);

    store(1, wake_ok);
    store(2, probe);
    store(0, MAGIC);

    if wake_ok == 6 && probe.abs_diff(PROBE_COUNTS) < PROBE_COUNTS / 10 {
        defmt::info!(
            "wake {}/6, {} ms delay = {} cycles, SELFTEST PASS",
            wake_ok,
            PROBE_MS,
            probe
        );
    } else {
        defmt::error!(
            "wake {}/6, {} ms delay = {} cycles, SELFTEST FAIL",
            wake_ok,
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
