//! Low-power modes: halt and sleep.
//!
//! CH585/CH584 (datasheet 5.3) and CH592/CH595 (datasheet 5.1) provide four
//! power modes. This module implements the two that keep the program running:
//!
//! | Mode | Core | Clock | Main LDO | Typical wake time |
//! | --- | --- | --- | --- | --- |
//! | [`halt`] | stopped | stopped, restarts on wake | on | 150 us |
//! | [`sleep`] | stopped | off, restarts on wake | off, ULP-LDO supplies the core | 400 us |
//!
//! Shutdown ends in a PMU reset (`RB_RESET_FLAG = GRWSM`) instead of a return,
//! so it is not part of this driver.
//!
//! # Entering and leaving
//!
//! [`halt`] sets `SLEEPDEEP`, translates the requested wake sources into
//! `SLP_WAKE_CTRL`, and executes `wfi`. [`sleep`] additionally writes a
//! `POWER_PLAN` that turns the main LDO off and powers only the domains the
//! caller retains. Both restore every register they touched, including
//! `SLEEPDEEP`, so the caller and any driver that runs afterwards see the
//! configuration they had before.
//!
//! Interrupts stay masked for the whole transition: the core resumes at the
//! configured clock before any handler runs. The wake is a PMU event, not a
//! line, so no vector needs to be enabled; the PFIC masks are dropped again
//! before the call returns and a peripheral whose flag set during the sleep is
//! then serviced normally.
//!
//! `wfi` runs as `wfe` on this core (the vendor runtime sets `WFITOWFE`), and
//! a wake leaves the event register set. Each entry therefore starts with a
//! `SETEVENT` write plus a `wfe` that cannot block, which empties the register;
//! without it every other sleep returns immediately.
//!
//! # Why this does not copy WCH's sequence
//!
//! WCH's `LowPower_Halt`/`LowPower_Sleep` switch `CLK_CFG` to a 4 MHz
//! oscillator and patch `FLASH_CFG`/`FLASH_SCK` for that clock on
//! CH585/CH584, because their wake path is copied to RAM (`__HIGH_CODE`) and
//! FlashROM is powered down; the CH59x versions keep the clock but reset the
//! FlashROM controller and power the FlashROM down the same way. This driver
//! leaves `CLK_CFG`, the FlashROM controller and the FlashROM power alone: the
//! PMU restarts the configured oscillators and applies the programmed wake
//! delay before the core resumes (datasheet 5.2 and 22.5, `TSUCLK` includes
//! the HSE and PLL startup time), so execution continues in FlashROM and
//! [`crate::rcc::clocks`] stays valid. The price is current: the clock tree
//! keeps its full speed in halt, and `sleep` retains `PWR_XROM` instead of
//! powering the FlashROM down.
//!
//! # Timekeeping
//!
//! The core clock stops in both modes, so `embassy-time`, [`crate::delay`] and
//! any timer clocked from `Fsys` lose the sleep interval. The RTC keeps
//! counting through the internal 32 kHz oscillator; use it
//! ([`crate::rtc::Rtc::cycle_count`]) to measure how long the chip slept. An
//! interval timer that was running resumes where it stopped.
//!
//! A wake from `sleep` also runs the PMU's local reset of the registers that
//! lost power (datasheet 4.1.6, `LRW`), and SysTick is among them: its control
//! register reads back 0, which turns every [`crate::delay::Delay`] call into
//! an endless spin. `sleep` saves and restores the SysTick block for that
//! reason, so the caller's timebase keeps running across the call and only the
//! sleep interval itself is lost.
//!
//! # Wake sources
//!
//! * [`WakeSources::RTC`] follows the RTC trigger, which
//!   [`crate::rtc::Rtc::set_alarm_in`] arms.
//! * [`WakeSources::GPIO`] follows the GPIO port interrupt. A pin must be armed
//!   as an input with the port interrupt enabled; the port vector has to stay
//!   enabled, otherwise the source cannot assert.
//! * [`WakeSources::USB_FS`], [`WakeSources::USB_HS`] and
//!   [`WakeSources::BATTERY`] follow their controller events.
//! * [`WakeSources::NONE`] leaves `SLP_WAKE_CTRL` alone, so whatever source was
//!   selected last stays armed. With none armed at all the chip never wakes:
//!   pending interrupts do not end the `wfe`, because `SEVONPEND` is cleared
//!   and the pending bits are dropped.
//!
//! A wake source that is enabled but never asserts leaves the chip asleep; the
//! driver cannot time that out because every timer is stopped.

use core::ops::BitOr;

use crate::pac;
use crate::rcc;

/// Wake event sources, written to `SLP_WAKE_CTRL` for the sleep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakeSources(u8);

impl WakeSources {
    /// No event source; see the module docs.
    pub const NONE: Self = Self(0);
    /// Full-speed USB resume.
    pub const USB_FS: Self = Self(0x01);
    /// High-speed USB resume. CH592/CH595 have no USB2 wake bit and reject
    /// this source.
    pub const USB_HS: Self = Self(0x02);
    /// RTC trigger compare.
    pub const RTC: Self = Self(0x08);
    /// GPIO port interrupt.
    pub const GPIO: Self = Self(0x10);
    /// Battery low-voltage detector.
    pub const BATTERY: Self = Self(0x20);

    /// Whether no source is selected.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether `other` is one of the selected sources.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for WakeSources {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Number of clock cycles the PMU waits after a wake event before the core
/// resumes (`SLP_POWER_CTRL.WAKE_DLY_MOD`).
///
/// The datasheet (5.2) adds `TSUCLK` to every value; `TSUCLK` already contains
/// the HSE and PLL startup time. [`WakeDelay::Long`] is the safe choice while
/// `CLK_CFG` keeps a high-speed source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WakeDelay {
    /// 3590 cycles plus oscillator startup.
    Long = 0b00,
    /// 520 cycles plus oscillator startup.
    Short = 0b01,
    /// 70 cycles plus oscillator startup.
    VeryShort = 0b10,
    /// 8 cycles plus oscillator startup; the oscillator ramp is not covered.
    None = 0b11,
}

/// Power domains `sleep` keeps powered.
///
/// The core and the FlashROM are always retained by this driver, so the wake
/// path can execute from FlashROM. Everything else is optional.
///
/// The stack and statics live in the main SRAM, so a `sleep` that returns
/// normally needs the main block (bit 4); the linker does not separate the main
/// and retention SRAM blocks, so the retention block (bit 1) is needed as well
/// whenever data may have been placed in it. Keeping both, as WCH's example
/// does, is the safe choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention(u16);

impl Retention {
    /// No optional domain.
    pub const NONE: Self = Self(0);
    /// The retention SRAM block (bit 1, 32 KB on CH585/CH584).
    #[cfg(not(power_sleep_ch59x))]
    pub const RAM32K: Self = Self(1 << 1);
    /// The retention SRAM block (bit 1, 2 KB on CH592).
    #[cfg(all(power_sleep_ch59x, ch592))]
    pub const RAM2K: Self = Self(1 << 1);
    /// The retention SRAM block (bit 1, 24 KB on CH595).
    #[cfg(all(power_sleep_ch59x, ch595))]
    pub const RAM24K: Self = Self(1 << 1);
    /// The main SRAM block (bit 4, 96 KB on CH585/CH584).
    #[cfg(not(power_sleep_ch59x))]
    pub const RAM96K: Self = Self(1 << 4);
    /// The main SRAM block (bit 4, 24 KB on CH592).
    #[cfg(all(power_sleep_ch59x, ch592))]
    pub const RAM24K: Self = Self(1 << 4);
    /// The main SRAM block (bit 4, 8 KB on CH595).
    #[cfg(all(power_sleep_ch59x, ch595))]
    pub const RAM8K: Self = Self(1 << 4);
    /// USB and RF configuration.
    pub const USB_RF: Self = Self(1 << 3);

    /// Whether `other` is retained.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for Retention {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Stop the core until a wake event, with the clock tree stopped but powered.
///
/// Wake sources are enabled for the duration of the call and restored
/// afterwards. Only the selected PMU events end the sleep; with
/// [`WakeSources::NONE`] the previously configured sources stay in force.
///
/// Returns after the wake delay, with the clock tree, `SLEEPDEEP` and the wake
/// configuration back to what they were before the call.
pub fn halt(wake: WakeSources, delay: WakeDelay) {
    enter(wake, delay, None);
}

/// Stop the core with the main LDO off, until a wake event.
///
/// `retention` selects the domains that stay powered; see [`Retention`] for
/// what the wake path needs. The core and FlashROM are always retained.
///
/// See [`halt`] for the wake sources and what is restored.
pub fn sleep(retention: Retention, wake: WakeSources, delay: WakeDelay) {
    enter(wake, delay, Some(retention));
}

fn enter(wake: WakeSources, delay: WakeDelay, retention: Option<Retention>) {
    let sys = pac::SYS;
    let saved_sctlr = pac::PFIC.sctlr().read();
    let saved_wake = sys.slp_wake_ctrl().read();
    let saved_power = sys.slp_power_ctrl().read();
    #[cfg(any(not(power_sleep_ch59x), ch595))]
    let saved_hfck = sys.hfck_pwr_ctrl().read();
    let saved_bat = sys.bat_det_ctrl().read();
    let saved_tune = pac::SYSTEM_CONTROL.xt32m_tune().read();
    let saved_plan = sys.power_plan().read();
    let saved_enabled = (
        pac::PFIC.isr1().read().0,
        pac::PFIC.isr2().read().0,
    );
    // The wake reset (LRW, datasheet 4.1.6) clears the counter registers of
    // the blocks that lost power, and SysTick is one of them: after `sleep`
    // its control register reads back 0 and every `Delay` busy-waits forever.
    // Save the block here and put it back, so the caller's timebase keeps
    // running (it loses the sleep interval, which no counter in the core can
    // see anyway).
    let saved_stk = (
        pac::SYSTICK.stk_ctlr().read().0,
        pac::SYSTICK.stk_cntl().read().0,
        pac::SYSTICK.stk_cnth().read().0,
        pac::SYSTICK.stk_cmplr().read().0,
        pac::SYSTICK.stk_cmphr().read().0,
    );

    critical_section::with(|_| {
        // A wake leaves the core's event register set, and the next `wfi`
        // (which this driver runs as `wfe`) then returns at once without
        // sleeping; every second sleep would end immediately. `SETEVENT` sets
        // that register, so a `wfe` with `SLEEPDEEP` clear consumes it here and
        // cannot block. Done before the power plan is armed, so the idle `wfi`
        // cannot execute the plan.
        let mut drain = saved_sctlr;
        drain.set_sleepdeep(false);
        drain.set_wfitowfe(true);
        drain.set_setevent(true);
        pac::PFIC.sctlr().write_value(drain);
        unsafe { core::arch::asm!("wfi", options(nostack, preserves_flags)) };

        // Battery monitoring adds current and does not survive the shutdown of
        // its analog block. The crystal gets its 150% bias current so the HSE
        // restarts reliably after the clock has stopped, as WCH's driver does.
        rcc::with_safe_access(|| {
            sys.bat_det_ctrl()
                .write_value(pac::sys::regs::BatDetCtrl(0));
            pac::SYSTEM_CONTROL
                .xt32m_tune()
                .modify(|w| w.set_xt32m_i_bias(0b11));
        });

        if !wake.is_empty() {
            // CH592/CH595 have no USB2 wake bit, so reject the source instead
            // of silently ignoring it.
            #[cfg(power_sleep_ch59x)]
            assert!(
                !wake.contains(WakeSources::USB_HS),
                "this chip has no high-speed USB wake source"
            );
            rcc::with_safe_access(|| {
                let mut value = pac::sys::regs::SlpWakeCtrl(0);
                // Remember short pulses; a button press can be shorter than the
                // wake latency.
                value.set_wake_ev_mode(true);
                value.set_slp_usb_wake(wake.contains(WakeSources::USB_FS));
                #[cfg(not(power_sleep_ch59x))]
                value.set_slp_usb2_wake(wake.contains(WakeSources::USB_HS));
                value.set_slp_rtc_wake(wake.contains(WakeSources::RTC));
                value.set_slp_gpio_wake(wake.contains(WakeSources::GPIO));
                value.set_slp_bat_wake(wake.contains(WakeSources::BATTERY));
                sys.slp_wake_ctrl().write_value(value);
            });
            rcc::with_safe_access(|| {
                sys.slp_power_ctrl()
                    .modify(|w| w.set_wake_dly_mod(delay as u8));
            });
        }

        if let Some(retention) = retention {
            rcc::with_safe_access(|| {
                #[cfg(power_sleep_ch59x)]
                {
                    // WCH's `LowPower_Sleep`: keep the DC-DC mode bits as they
                    // are, retain the core, FlashROM and the requested SRAM /
                    // USB-RF domains, and select the 1.406 ms HSE pre-wake
                    // setting. `PWR_SYS_EN` and `PWR_LDO_EN` stay 0, which
                    // turns the main LDO off and lets the ULP LDO supply the
                    // retained domains.
                    let mut plan = saved_plan.0 & (0x0200 | 0x0400);
                    plan |= 0x8000 // PWR_PLAN_EN
                        | 0x0004 // PWR_CORE
                        | 0x0001 // PWR_XROM
                        | (0b10 << 11); // XT_PRE_CFG
                    plan |= retention.0 & (0x0002 | 0x0010 | 0x0008);
                    sys.power_plan().write_value(pac::sys::regs::PowerPlan(plan));
                    // CH592 has a single low-retention-voltage bit. CH595
                    // encodes a 2-bit voltage level there whose values the
                    // metadata does not define, so leave it at the reset value
                    // (normal voltage) instead of guessing.
                    #[cfg(ch592)]
                    sys.slp_power_ctrl().modify(|w| w.set_ram_ret_lv(true));
                    // CH595 has the CH585-style high-frequency clock power
                    // block: the PMU runs its wake logic from the internal RC
                    // oscillator, so it has to stay powered while the PLL and
                    // the crystal stop.
                    #[cfg(ch595)]
                    sys.hfck_pwr_ctrl().modify(|w| w.set_clk_rc16m_pon(true));
                }
                #[cfg(not(power_sleep_ch59x))]
                {
                    sys.power_plan().write(|w| {
                        // The DC-DC mode is immediate, not a plan bit: keep it.
                        w.set_pwr_dcdc_pre(saved_plan.pwr_dcdc_pre());
                        w.set_pwr_dcdc_en(saved_plan.pwr_dcdc_en());
                        // Reserved, the datasheet requires 0010b (5.2).
                        w.set_pwr_must_0010(0b0010);
                        w.set_pwr_core(true);
                        w.set_pwr_xrom(true);
                        w.set_pwr_ram32k(retention.contains(Retention::RAM32K));
                        w.set_pwr_ram96k(retention.contains(Retention::RAM96K));
                        w.set_pwr_extend(retention.contains(Retention::USB_RF));
                        w.set_pwr_plan_en(true);
                    });
                    // The PMU runs its wake logic from the internal RC oscillator,
                    // so it has to stay powered while the PLL and HSE stop.
                    sys.hfck_pwr_ctrl().modify(|w| w.set_clk_rc16m_pon(true));
                }
            });
        }

        let mut sctlr = saved_sctlr;
        sctlr.set_setevent(false);
        sctlr.set_sevonpend(false);
        sctlr.set_sleepdeep(true);
        // Run `wfi` as `wfe`: the PMU wake sources are events, and none of them
        // needs a PFIC line to be enabled.
        sctlr.set_wfitowfe(true);
        pac::PFIC.sctlr().write_value(sctlr);

        // A pending interrupt ends `wfe` at once, and the RTC line is already
        // pending as soon as its trigger is armed. WCH masks every line before
        // a deep sleep for this reason; the PMU wake sources above are
        // peripheral events and do not need the PFIC. The enable state is
        // restored below.
        pac::PFIC.irer1().write(|w| w.0 = 0xffff_ffff);
        pac::PFIC.irer2().write(|w| w.0 = 0xffff_ffff);
        // A line that is still pending from before the call ends the `wfe`
        // at once. Drop those latches; the peripheral flag itself is what the
        // caller armed.
        pac::PFIC.iprr1().write(|w| w.0 = 0xffff_ffff);
        pac::PFIC.iprr2().write(|w| w.0 = 0xffff_ffff);

        // With `SEVONPEND` cleared, no line pending and the stale event
        // drained above, only the PMU wake sources end this.
        unsafe { core::arch::asm!("wfi", options(nostack, preserves_flags)) };

        // Restore the lines before the wake registers, so a flag that set
        // during the sleep is serviced with the configuration it expects.
        pac::PFIC.ienr1().write(|w| w.0 = saved_enabled.0);
        pac::PFIC.ienr2().write(|w| w.0 = saved_enabled.1);

        // Clear `SLEEPDEEP` before anything else: the Embassy executor's idle
        // loop also executes `wfi`, and would otherwise re-enter deep sleep.
        // `SETEVENT` is write-1-to-send; never write the bit back.
        let mut restored = saved_sctlr;
        restored.set_setevent(false);
        pac::PFIC.sctlr().write_value(restored);

        // The PMU has already waited for the oscillators, but a few cycles
        // guard the SAM window that follows.
        for _ in 0..64 {
            core::hint::spin_loop();
        }

        rcc::with_safe_access(|| {
            pac::SYSTEM_CONTROL
                .xt32m_tune()
                .write_value(saved_tune);
            sys.bat_det_ctrl().write_value(saved_bat);
            sys.slp_wake_ctrl().write_value(saved_wake);
            sys.slp_power_ctrl().write_value(saved_power);
            #[cfg(any(not(power_sleep_ch59x), ch595))]
            sys.hfck_pwr_ctrl().write_value(saved_hfck);
            if retention.is_some() {
                // The plan has executed; WCH's driver only clears the enable
                // bit, leaving the plan itself for the next entry to recompose.
                sys.power_plan().modify(|w| w.set_pwr_plan_en(false));
            }
        });

        if retention.is_some() {
            pac::SYSTICK.stk_cnth().write_value(pac::systick::regs::StkCnth(saved_stk.2));
            pac::SYSTICK.stk_cntl().write_value(pac::systick::regs::StkCntl(saved_stk.1));
            pac::SYSTICK.stk_cmphr().write_value(pac::systick::regs::StkCmphr(saved_stk.4));
            pac::SYSTICK.stk_cmplr().write_value(pac::systick::regs::StkCmplr(saved_stk.3));
            pac::SYSTICK.stk_ctlr().write_value(pac::systick::regs::StkCtlr(saved_stk.0));
        }
    });
}
