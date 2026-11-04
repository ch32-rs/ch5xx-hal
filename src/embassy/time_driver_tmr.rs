//! Embassy time driver on a TMRx peripheral.
//!
//! Selected by one of the `time-driver-tmr0` … `time-driver-tmr3` features.
//! All four CH585 timers have local DMA. NFC PICC reserves TMR0/TMR3 and WCH's
//! BLE examples also use TMR3; choose the timebase for the application's needs.
//!
//! # Why use this instead of SysTick
//!
//! SysTick is the core's timer, so the [`super::time_driver_systick`] driver
//! takes it away from [`crate::delay`] and from anything else that wants a
//! cycle counter. A TMR instance is a normal peripheral: the blocking `delay`
//! keeps working, and the timebase can be handed a wider free-running counter
//! that other code may read.
//!
//! # The peripheral
//!
//! CH585 has four 26-bit up counters (`TMR0`–`TMR3`). Unlike the
//! STM32/CH32 general-purpose timers there is **no prescaler**: the counter runs
//! at the configured nominal system clock (~5.33 MHz at reset). The timebase
//! stores that rate rather than baking it in as a shift; RTC calibration is
//! used separately by the blocking delay.
//!
//! Measured behaviour, in an earlier throwaway probe (ratio 1.00 against
//! SysTick; `max_before = 19998`, `min_after = 2` across a `CNT_END` of 20000):
//!
//!
//! - `CTRL_MOD.ALL_CLEAR` is a **held level**, not a self-clearing strobe: set
//!   it, program `CNT_END`, then release it. Pulsing it in one write leaves the
//!   counter stuck at 0.
//! - With `COUNT_EN` set the counter counts to `CNT_END` and reloads at 0.
//! - Reaching `CNT_END` sets `INT_FLAG.IF_CYC_END`; the flag is `RW1`
//!   (write 1 clears), unlike SysTick's `RW0` `CNTIF`.
//! - `COUNT` is 26 bits wide; the upper bits of the word read as zero.
//!
//! The reload model and the 64-bit timebase are shared with the SysTick driver
//! — see [`super::reload_timer`].

use super::reload_timer::{ReloadState, ReloadTimer};
use crate::interrupt::InterruptExt;
use crate::pac;

// Which instance this file drives. Enabling two `time-driver-tmr*` features is
// already a `compile_error!` in `super`, but these arms are mutually exclusive
// too so that build reports that one message rather than a duplicate-definition
// wall from `tmr()`/`IRQ`.

/// The TMR instance this build drives.
#[inline(always)]
fn tmr() -> &'static crate::pac::tmr::TmrFull {
    #[cfg(feature = "time-driver-tmr0")]
    {
        &crate::pac::TMR0
    }
    #[cfg(all(not(feature = "time-driver-tmr0"), feature = "time-driver-tmr1"))]
    {
        &crate::pac::TMR1
    }
    #[cfg(all(
        not(feature = "time-driver-tmr0"),
        not(feature = "time-driver-tmr1"),
        feature = "time-driver-tmr2"
    ))]
    {
        &crate::pac::TMR2
    }
    #[cfg(all(
        not(feature = "time-driver-tmr0"),
        not(feature = "time-driver-tmr1"),
        not(feature = "time-driver-tmr2"),
        feature = "time-driver-tmr3"
    ))]
    {
        &crate::pac::TMR3
    }
}

/// The interrupt vector matching the selected instance.
#[cfg(feature = "time-driver-tmr0")]
const IRQ: pac::Interrupt = pac::Interrupt::TMR0;
#[cfg(all(not(feature = "time-driver-tmr0"), feature = "time-driver-tmr1"))]
const IRQ: pac::Interrupt = pac::Interrupt::TMR1;
#[cfg(all(
    not(feature = "time-driver-tmr0"),
    not(feature = "time-driver-tmr1"),
    feature = "time-driver-tmr2"
))]
const IRQ: pac::Interrupt = pac::Interrupt::TMR2;
#[cfg(all(
    not(feature = "time-driver-tmr0"),
    not(feature = "time-driver-tmr1"),
    not(feature = "time-driver-tmr2"),
    feature = "time-driver-tmr3"
))]
const IRQ: pac::Interrupt = pac::Interrupt::TMR3;

/// TMRx as seen by [`ReloadState`].
pub(crate) struct TmrTimer;

impl ReloadTimer for TmrTimer {
    const RESETS_ON_SET_END: bool = true;
    /// `COUNT`/`CNT_END` are 26 bits.
    const MASK: u32 = 0x03FF_FFFF;

    #[inline]
    fn count(&self) -> u32 {
        tmr().count().read().count()
    }

    #[inline]
    fn set_end(&self, end: u32) {
        tmr().cnt_end().write(|w| w.set_cnt_end(end));
    }

    #[inline]
    fn clear_flag(&self) {
        // `INT_FLAG` is RW1: writing 1 clears the bit.
        tmr().int_flag().write(|w| w.set_if_cyc_end(true));
    }
}

struct Driver {
    state: ReloadState,
    timer: TmrTimer,
}

embassy_time_driver::time_driver_impl!(static DRIVER: Driver = Driver {
    state: ReloadState::new::<TmrTimer>(),
    timer: TmrTimer,
});

impl embassy_time_driver::Driver for Driver {
    fn now(&self) -> u64 {
        self.state.ticks_to_time(self.state.ticks(&self.timer))
    }

    fn schedule_wake(&self, at: u64, waker: &core::task::Waker) {
        self.state.schedule_wake(&self.timer, at, waker);
    }
}

/// The selected TMR's interrupt vector.
///
/// `qingke-rt`'s `#[interrupt]` macro needs a literal name, so all four are
/// declared and the unused ones are cfg'd out.
macro_rules! tmr_irq {
    ($($name:ident => $feature:literal),* $(,)?) => {
        $(
            #[cfg(feature = $feature)]
            #[qingke_rt::interrupt]
            fn $name() {
                on_interrupt();
            }
        )*
    };
}

tmr_irq! {
    TMR0 => "time-driver-tmr0",
    TMR1 => "time-driver-tmr1",
    TMR2 => "time-driver-tmr2",
    TMR3 => "time-driver-tmr3",
}

fn on_interrupt() {
    if !tmr().int_flag().read().if_cyc_end() {
        // Not ours. Leaving the flag alone; there is nothing else to service.
        return;
    }
    tmr().int_flag().write(|w| w.set_if_cyc_end(true));

    critical_section::with(|cs| {
        DRIVER.state.on_period(cs, &DRIVER.timer);
    });
}

/// Hand a TMR instance to the embassy time driver.
///
/// # Safety
///
/// Call exactly once, after [`crate::rcc`] has a measured clock. The selected
/// TMR is reprogrammed from scratch, so any other user of that instance must
/// be gone.
pub unsafe fn init() {
    let hz = crate::rcc::clocks().sysclk.0;
    debug_assert!(hz != 0, "embassy time driver initialised before the clock");

    // Stop and reset the counter. `ALL_CLEAR` is a held level: it must be set
    // before the end value is programmed and released afterwards, or the
    // counter stays at 0.
    tmr().ctrl_mod().write(|w| w.set_all_clear(true));
    tmr()
        .cnt_end()
        .write(|w| w.set_cnt_end(<TmrTimer as ReloadTimer>::MAX_PERIOD - 1));
    tmr().int_flag().write(|w| w.set_if_cyc_end(true));
    tmr().inter_en().write(|w| w.set_ie_cyc_end(true));

    critical_section::with(|cs| DRIVER.state.reset::<TmrTimer>(cs, hz));

    // Release `ALL_CLEAR` and start counting. `MODE_IN` and the PWM/capture
    // fields keep their reset value (timer mode), so this write leaves them out.
    tmr().ctrl_mod().modify(|w| {
        w.set_all_clear(false);
        w.set_count_en(true);
    });

    // TMR interrupts *are* in the generated table, so this goes through the
    // typelevel API like any other HAL driver would.
    IRQ.set_priority(super::TIME_DRIVER_PRIORITY);
    unsafe { IRQ.enable() };

    super::enable_interrupts();
}
