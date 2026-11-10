//! Walk the CH58x clock tree at runtime and check every step against SysTick.
//!
//! The CH58x has a 480 MHz PLL fed by the 32 MHz crystal, so this exercises the
//! vendor `SetSysClock` sequence: the `PLL_CONFIG` bit 5/7 handshake, the three
//! Flash timing values, and the oscillator power bits in `HFCK_PWR_CTRL`.
//!
//! Each step caches the new nominal clock, re-measures the SysTick rate against
//! the RTC, then busy-waits one second and compares the counter delta with the
//! nominal frequency. A step passes when the delta is within 25 % of nominal,
//! which covers the 32 kHz RC reference error and any PLL rounding.
//!
//! This is a coarse check against the RTC's oscillator, not an independent
//! measurement of delay accuracy: the delay and its SysTick delta both use the
//! same calibration. Runtime transitions remain under investigation.
//!
//! ```console
//! PROBE_RS_PROBE=1a86:8010:2FC48F06B4A8 cargo run --release --bin clock_ladder
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::{pac, rcc};
use defmt::{error, info};
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

const LADDER: &[(&str, rcc::Config)] = &[
    ("reset 6.4 MHz", rcc::Config::HSE_6_4MHZ),
    ("HSE 8 MHz", rcc::Config::HSE_8MHZ),
    ("HSE 16 MHz", rcc::Config::HSE_16MHZ),
    ("PLL 24 MHz", rcc::Config::PLL_24MHZ),
    ("PLL 32 MHz", rcc::Config::PLL_32MHZ),
    ("PLL 48 MHz", rcc::Config::PLL_48MHZ),
    ("PLL 60 MHz", rcc::Config::PLL_60MHZ),
    ("PLL 80 MHz", rcc::Config::PLL_80MHZ),
    ("HSE 6.4 MHz", rcc::Config::HSE_6_4MHZ),
];

#[entry]
fn main() -> ! {
    let _p = hal::init(Default::default());
    let mut delay = hal::delay::Delay;
    let mut failures = 0u32;

    for &(name, config) in LADDER {
        let wanted = config.sysclk().0;
        let clocks = unsafe { rcc::set_config(config) };
        unsafe { hal::delay::recalibrate() };

        let measured = hal::delay::measured_sysclk().0;
        let start = pac::SYSTICK.stk_cntl().read().cntl();
        delay.delay_ms(1000);
        let end = pac::SYSTICK.stk_cntl().read().cntl();
        let delta = end.wrapping_sub(start);

        let ratio_ok = delta.abs_diff(wanted) <= wanted / 4;
        info!(
            "{}: nominal {} Hz, cached {} Hz, measured {} Hz, delta {} -> {}",
            name,
            wanted,
            clocks.sysclk.0,
            measured,
            delta,
            if ratio_ok { "PASS" } else { "FAIL" }
        );
        if clocks.sysclk.0 != wanted || !ratio_ok {
            failures += 1;
        }

        // Let RTT drain at the slower clocks before reconfiguring again.
        delay.delay_ms(100);
    }

    if failures == 0 {
        info!("clock ladder SELFTEST PASS");
    } else {
        error!("clock ladder SELFTEST FAIL: {} step(s)", failures);
    }

    loop {
        delay.delay_ms(1000);
    }
}
