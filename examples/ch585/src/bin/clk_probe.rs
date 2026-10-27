//! Read the clock control registers from firmware, not from the debugger.
//!
//! `probe-rs read` halts the core to inspect memory, so the values it reports
//! are not necessarily what the running firmware sees. This binary reads the
//! same registers in software and prints them over RTT, which is the only
//! trustworthy view of the boot clock setup.
//!
//! It uses `Config::default()`, i.e. the datasheet reset value. See `clk_set`
//! for the same report after configuring a different tree.
//!
//! ```console
//! cargo run --release --bin clk_probe
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
use defmt::info;
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let clk = pac::SYS.clk_cfg().read();
    let hfck = pac::SYS.hfck_pwr_ctrl().read();
    let safe_mode = pac::SYS.safe_mode_ctrl().read();
    let sig = pac::SYSTEM_CONTROL.safe_access_sig().read();
    let pll_cfg = pac::SYSTEM_CONTROL.pll_config().read();
    let xt32m = pac::SYSTEM_CONTROL.xt32m_tune().read();

    info!("CLK_CFG        = {=u16:#06x}", clk.0);
    info!("  sys_mod      = {}", clk.clk_sys_mod());
    info!("  pll_div      = {}", clk.clk_pll_div());
    info!("  osc32m_sel   = {}", clk.osc32m_sel());
    info!("  xrom_sclk    = {}", clk.xrom_sclk_sel());
    info!("HFCK_PWR_CTRL  = {=u8:#04x}", hfck.0);
    info!("  rc16m_pon    = {}", hfck.clk_rc16m_pon());
    info!("  xt32m_pon    = {}", hfck.clk_xt32m_pon());
    info!("  pll_pon      = {}", hfck.clk_pll_pon());
    info!("SAFE_MODE_CTRL = {=u8:#04x}", safe_mode.0);
    info!("SAFE_ACCESS_SIG= {=u8:#04x}", sig.0);
    info!("PLL_CONFIG     = {=u8:#04x}", pll_cfg.0);
    info!("XT32M_TUNE     = {=u8:#04x}", xt32m.0);
    info!("nominal sysclk  = {} Hz", hal::rcc::clocks().sysclk.0);
    info!("measured sysclk = {} Hz", hal::delay::measured_sysclk().0);

    // SysTick is already free-running from `init`. Count it for a fixed number
    // of 32 kHz RTC ticks, so the ratio is visible without a debugger read.
    let mut delay = hal::delay::Delay;
    let start = pac::SYSTICK.stk_cntl().read().cntl();
    delay.delay_ms(1000);
    let end = pac::SYSTICK.stk_cntl().read().cntl();

    info!("systick delta over 1s = {}", end.wrapping_sub(start));

    let _ = p;

    loop {
        core::hint::spin_loop();
    }
}
