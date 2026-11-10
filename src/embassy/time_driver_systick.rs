//! Embassy time driver on the core SysTick counter.
//!
//! # Why this is not the CH32 driver
//!
//! CH5xx's SysTick is the WCH variant. The 32-bit dies (CH572/CH570,
//! CH581/CH584/CH585 and CH595) use `CTLR` (`STE`, `STIE`, `STCLK`, `STRE`,
//! `MODE`), `SR.CNTIF`, `CNTL` and `CMPLR`; their `CNTH`/`CMPHR` words are
//! reserved and must not be used. The 64-bit dies (CH582/CH583, CH591/CH592)
//! have real high words, but this driver still uses the low 32-bit pair with
//! the same up-counter/auto-reload model, and never writes `CMPHR`.
//!
//! Measured behaviour:
//!
//! - With `STRE = 1` the counter counts up to `CMPLR` and restarts at 0.
//! - Reaching the compare value sets `SR.CNTIF`, which stays set until
//!   written to 0 (writing 1 is a no-op, so this is `RW0`, not `RW1`).
//! - `CNTL` can be written while the counter is stopped and resumes from there.
//!
//! # How 64 bits are built
//!
//! The counter reloads at the compare value, so every interrupt advances the
//! software high part by *exactly* the period that was programmed. The 64-bit
//! tick count is `high + CNTL`, where `high` is the tick value at the start of
//! the current hardware period. That makes the timebase exact: no drift from
//! interrupt latency, because the hardware — not the ISR — decides when the
//! period ends.
//!
//! The compare value is programmed for the next alarm, not on a fixed
//! schedule, so the driver takes one interrupt per alarm instead of one per
//! millisecond. A far-away deadline is split at `MAX_PERIOD` so `high` still
//! advances during long idle stretches.
//!
//! `super::reload_timer` holds the state machine this shares with the TMR
//! driver; this file is only the SysTick register access.
//!
//! # Why the counter is re-based on init
//!
//! The blocking [`crate::delay`] driver starts SysTick as a free-running
//! counter with `STRE = 0`. Embassy needs `STRE = 1` and its own compare value,
//! so [`init`] reprograms the counter from scratch. Enabling a `time-driver-*`
//! feature therefore takes SysTick away from `delay`: both cannot own it.
//!
//! # No atomic read-modify-write
//!
//! The CH585's QingKe V3C core raises a hardware exception on `amoadd.w`, so
//! **no `fetch_add` / `compare_exchange` may be used anywhere**. Everything
//! shared here is therefore either a plain load/store or lives inside a
//! [`critical_section`].

use super::reload_timer::{ReloadState, ReloadTimer};
use crate::pac::SYSTICK;

/// SysTick as seen by [`ReloadState`].
pub(crate) struct SystickTimer;

impl ReloadTimer for SystickTimer {
    const MASK: u32 = u32::MAX;

    #[inline]
    fn count(&self) -> u32 {
        SYSTICK.cntl().read().cntl()
    }

    #[inline]
    fn set_end(&self, end: u32) {
        set_compare(end);
    }

    #[inline]
    fn clear_flag(&self) {
        clear_count_flag();
    }
}

/// Program the compare/reload value.
#[inline]
fn set_compare(end: u32) {
    SYSTICK.cmplr().write(|w| w.set_cmpl(end));
}

/// Drop a latched compare flag.
#[inline]
fn clear_count_flag() {
    #[cfg(systick_sr)]
    SYSTICK.sr().modify(|w| w.set_cntif(false));
    #[cfg(systick_cntfg)]
    SYSTICK.cntfg().modify(|w| w.set_cntif(false));
}

/// Whether the compare flag is set.
#[inline]
fn count_flag() -> bool {
    cfg_select! {
        systick_sr => SYSTICK.sr().read().cntif(),
        systick_cntfg => SYSTICK.cntfg().read().cntif(),
        _ => false,
    }
}

struct Driver {
    state: ReloadState,
    timer: SystickTimer,
}

embassy_time_driver::time_driver_impl!(static DRIVER: Driver = Driver {
    state: ReloadState::new::<SystickTimer>(),
    timer: SystickTimer,
});

impl embassy_time_driver::Driver for Driver {
    fn now(&self) -> u64 {
        self.state.ticks_to_time(self.state.ticks(&self.timer))
    }

    fn schedule_wake(&self, at: u64, waker: &core::task::Waker) {
        self.state.schedule_wake(&self.timer, at, waker);
    }
}

/// SysTick's core-interrupt vector (core interrupt 12).
#[qingke_rt::interrupt(core)]
fn SysTick() {
    if !count_flag() {
        // Not ours. Leaving `CNTIF` alone; there is nothing else to service.
        return;
    }
    clear_count_flag();

    critical_section::with(|cs| {
        DRIVER.state.on_period(cs, &DRIVER.timer);
    });
}

/// Hand SysTick to the embassy time driver.
///
/// # Safety
///
/// Call exactly once, after [`crate::rcc`] has a measured clock. It reprograms
/// SysTick, so the blocking [`crate::delay`] driver must not be used
/// afterwards.
pub unsafe fn init() {
    let hz = crate::rcc::clocks().sysclk.0;
    debug_assert!(hz != 0, "embassy time driver initialised before the clock");

    let stk = &SYSTICK;

    // Stop and rebase before changing the wrap point, so no stale compare
    // value can fire during the transition.
    stk.ctlr().modify(|w| w.set_ste(false));
    clear_count_flag();
    stk.cntl().write(|w| w.set_cntl(0));
    set_compare(<SystickTimer as ReloadTimer>::MAX_PERIOD - 1);

    critical_section::with(|cs| DRIVER.state.reset::<SystickTimer>(cs, hz));

    stk.ctlr().modify(|w| {
        #[cfg(systick_mode)]
        w.set_mode(false); // up-count
        w.set_stclk(true); // HCLK, not HCLK/8
                           // Reload at the compare value. CH573 calls the bit `STRELOAD`.
        #[cfg(systick_stre)]
        w.set_stre(true);
        #[cfg(systick_streload)]
        w.set_streload(true);
        w.set_stie(true); // interrupt on compare
        w.set_ste(true);
    });

    // SysTick is a *core* interrupt, so it is not in the external interrupt
    // table the generated `interrupt::typelevel` types come from, and there is
    // no `interrupt::typelevel::SysTick` to call. qingke's PFIC API is the only
    // route; the priority still goes through `Priority` rather than a magic
    // number.
    //
    // The level and its meaning are documented on `TIME_DRIVER_PRIORITY`.
    let irq = qingke_rt::CoreInterrupt::SysTick as u8;
    unsafe {
        qingke::pfic::set_priority(irq, super::TIME_DRIVER_PRIORITY as u8);
        qingke::pfic::enable_interrupt(irq);
    }

    super::enable_interrupts();
}
