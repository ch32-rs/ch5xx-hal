//! Embassy time driver on the core SysTick counter.
//!
//! # Why this is not the CH32 driver
//!
//! CH5xx's SysTick is a WCH variant with a **32-bit** counter:
//! `STK_CTLR` (`STE`, `STIE`, `STCLK`, `STRE`, `MODE`), `STK_SR.CNTIF`,
//! `STK_CNTL` and `STK_CMPLR`. The `STK_CNTH`/`STK_CMPHR` registers the PAC
//! exposes are the padding words at +0x0C and +0x14 — reading them returns the
//! neighbour, not a real high half. In particular the driver must not use
//! `stk_cmph().write_value(next >> 32)`: on the real part that writes into
//! `STK_CMPLR`'s neighbourhood.
//!
//! Measured behaviour:
//!
//! - With `STRE = 1` the counter counts up to `STK_CMPLR` and restarts at 0.
//! - Reaching the compare value sets `STK_SR.CNTIF`, which stays set until
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
//! millisecond. A far-away deadline is split at [`MAX_PERIOD`] so `high` still
//! advances during long idle stretches.
//!
//! [`super::reload_timer`] holds the state machine this shares with the TMR
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
        SYSTICK.stk_cntl().read().cntl()
    }

    #[inline]
    fn set_end(&self, end: u32) {
        SYSTICK.stk_cmplr().write(|w| w.set_cmpl(end));
    }

    #[inline]
    fn clear_flag(&self) {
        SYSTICK.stk_sr().modify(|w| w.set_cntif(false));
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
    let stk = &SYSTICK;
    if !stk.stk_sr().read().cntif() {
        // Not ours. Leaving `CNTIF` alone; there is nothing else to service.
        return;
    }
    stk.stk_sr().modify(|w| w.set_cntif(false));

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
    stk.stk_ctlr().modify(|w| w.set_ste(false));
    stk.stk_sr().modify(|w| w.set_cntif(false));
    stk.stk_cntl().write(|w| w.set_cntl(0));
    stk.stk_cmplr()
        .write(|w| w.set_cmpl(<SystickTimer as ReloadTimer>::MAX_PERIOD - 1));

    critical_section::with(|cs| DRIVER.state.reset::<SystickTimer>(cs, hz));

    stk.stk_ctlr().modify(|w| {
        w.set_mode(false); // up-count
        w.set_stclk(true); // HCLK, not HCLK/8
        w.set_stre(true); // reload at the compare value
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
