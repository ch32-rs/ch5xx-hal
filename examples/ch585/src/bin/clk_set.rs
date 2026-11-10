//! Boot with a configured system clock instead of the reset default.
//!
//! `Config::default()` is the datasheet reset value, HSI 16 MHz / 3 =
//! 5.333 MHz. This binary asks for `rcc::Config::HSI_PLL_24MHZ` instead: the
//! 312 MHz PLL divided by 13, using the internal RC oscillator only.
//!
//! 24 MHz makes 115200 baud usable without a crystal: the UART divider becomes
//! `DL = 26`, which is 115385 baud (+0.16%), inside the datasheet's 0.5%
//! transmit budget. 9600 baud is comfortable at any of these clocks.
//!
//! The report proves the switch from firmware itself: the `CLK_CFG` readback,
//! the nominal frequency cached by `rcc`, the rate `delay` measured against the
//! RTC, and the SysTick delta over a one-second wait.
//!
//! ```console
//! cargo run --release --bin clk_set
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::{pac, rcc};
use defmt::info;
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(rcc::Config::HSI_PLL_24MHZ.into());

    let clk = pac::SYS.clk_cfg().read();
    info!("CLK_CFG         = {=u16:#06x}", clk.0);
    info!("  sys_mod       = {}", clk.clk_sys_mod());
    info!("  pll_div       = {}", clk.clk_pll_div());
    info!("  osc32m_sel    = {}", clk.osc32m_sel());
    info!("  xrom_sclk     = {}", clk.xrom_sclk_sel());
    info!("nominal sysclk  = {} Hz", hal::rcc::clocks().sysclk.0);
    info!("measured sysclk = {} Hz", hal::delay::measured_sysclk().0);

    // An independent check of the rate: one second of busy-waiting must cover
    // about 24 million SysTick counts.
    let mut delay = hal::delay::Delay;
    let start = pac::SYSTICK.cntl().read().cntl();
    delay.delay_ms(1000);
    let end = pac::SYSTICK.cntl().read().cntl();

    info!("systick delta over 1s = {}", end.wrapping_sub(start));

    let _ = p;

    loop {
        core::hint::spin_loop();
    }
}
