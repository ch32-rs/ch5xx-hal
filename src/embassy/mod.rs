//! Embassy integration.
//!
//! One time driver is compiled in, chosen by a feature:
//!
//! | Feature | Driver | Takes |
//! | --- | --- | --- |
//! | `time-driver-systick` | `time_driver_systick.rs` | the core SysTick counter |
//! | `time-driver-systick` on CH573 | `time_driver_systick_ch573.rs` | the V3A down-counter SysTick |
//! | `time-driver-tmr0` … `time-driver-tmr3` | `time_driver_tmr.rs` | that TMR instance |
//!
//! CH573's SysTick is a 64-bit down-counter with a one-shot reload, so it
//! cannot share the up-counter/auto-reload driver. `build.rs` emits
//! `systick_downcounter` from the register metadata and the selection below
//! picks the matching module; a chip cannot silently use the wrong model.
//!
//! `time-driver-systick` is the lowest-latency option and is what the examples
//! use. It reprograms SysTick for its own reload model, which
//! [`crate::delay::Delay`] copes with by reading this driver's timebase rather
//! than the raw counter, so `delay` keeps working either way.
//!
//! Picking a `time-driver-tmr*` feature instead frees SysTick for anything else
//! that wants a free-running counter, and keeps `delay` on it too. TMR0 is the
//! least contended instance.
//!
//! # Ownership
//!
//! A TMR driver's peripheral is removed from [`crate::Peripherals`] by
//! `build.rs`, so [`crate::init`] can never hand the same instance to a second
//! user. SysTick is core-internal and was never in that struct.
//!
//! # Ticks per second
//!
//! [`embassy_time_driver::TICK_HZ`] is fixed at build time by embassy's
//! `tick-*` feature, while the CH585's system clock is measured at runtime.
//! The driver converts between the two using the calibrated rate from
//! [`crate::rcc::clocks`], so any `tick-hz-*` feature works; a rate that does
//! not divide the clock evenly loses sub-tick precision but never runs
//! backwards.

use crate::pac;

// The shared reload state machine drives the TMR instances and the up-counter
// SysTick. CH573's down-counter driver carries its own state, so it does not
// pull these in (and would otherwise leave them as dead code).
#[cfg(any(
    all(feature = "time-driver-systick", not(systick_downcounter)),
    feature = "time-driver-tmr0",
    feature = "time-driver-tmr1",
    feature = "time-driver-tmr2",
    feature = "time-driver-tmr3"
))]
mod reload_math;
#[cfg(any(
    all(feature = "time-driver-systick", not(systick_downcounter)),
    feature = "time-driver-tmr0",
    feature = "time-driver-tmr1",
    feature = "time-driver-tmr2",
    feature = "time-driver-tmr3"
))]
mod reload_timer;

#[cfg(not(any(
    feature = "time-driver-systick",
    feature = "time-driver-tmr0",
    feature = "time-driver-tmr1",
    feature = "time-driver-tmr2",
    feature = "time-driver-tmr3"
)))]
compile_error!(
    "the `embassy` feature needs exactly one time driver: enable `time-driver-systick` (the default) \
     or one of `time-driver-tmr0` … `time-driver-tmr3`"
);

#[cfg(all(
    feature = "time-driver-systick",
    any(
        feature = "time-driver-tmr0",
        feature = "time-driver-tmr1",
        feature = "time-driver-tmr2",
        feature = "time-driver-tmr3"
    )
))]
compile_error!("enable only one time driver: `time-driver-systick` or a single `time-driver-tmr*`");

// Enumerating the pairs is the only stable way to say "at most one of these
// four"; `cfg` has no counting.
#[cfg(any(
    all(feature = "time-driver-tmr0", feature = "time-driver-tmr1"),
    all(feature = "time-driver-tmr0", feature = "time-driver-tmr2"),
    all(feature = "time-driver-tmr0", feature = "time-driver-tmr3"),
    all(feature = "time-driver-tmr1", feature = "time-driver-tmr2"),
    all(feature = "time-driver-tmr1", feature = "time-driver-tmr3"),
    all(feature = "time-driver-tmr2", feature = "time-driver-tmr3"),
))]
compile_error!("enable only one `time-driver-tmr*` feature");

// CH573's V3A SysTick counts down and reloads from `CMP` only when software
// pulses `STRELOAD`; the shared driver's up-counter/auto-reload model is wrong
// there. Select the down-counter implementation from the capability cfg so a
// new die with the same block picks it up automatically.
#[cfg(all(feature = "time-driver-systick", systick_downcounter))]
#[path = "time_driver_systick_ch573.rs"]
pub mod time_driver_impl;

#[cfg(all(feature = "time-driver-systick", not(systick_downcounter)))]
#[path = "time_driver_systick.rs"]
pub mod time_driver_impl;

// `not(time-driver-systick)` only suppresses a duplicate `time_driver_impl` when
// both kinds are enabled — that case is already reported by the `compile_error!`
// above, and letting this module through too would bury it under `E0428`.
#[cfg(all(
    not(feature = "time-driver-systick"),
    any(
        feature = "time-driver-tmr0",
        feature = "time-driver-tmr1",
        feature = "time-driver-tmr2",
        feature = "time-driver-tmr3"
    )
))]
#[path = "time_driver_tmr.rs"]
pub mod time_driver_impl;

/// Priority given to the time driver's interrupt.
///
/// **On V3C only bits `[7:5]` of `PFIC_IPRIORx` are implemented**, and the
/// *QingKeV3 Processor Manual* states that the lower the value, the higher the
/// priority. So there are eight usable levels, 0…7, and `P8 = 0x80` is level 4:
/// mid-range, able to preempt ordinary work rather than the least urgent.
///
/// `qingke::interrupt::Priority` names 16 steps of `0x10`, but on this core each
/// pair (P0/P1, P2/P3, …) maps to the same hardware level. Do not read a 16-step
/// ordering into it; only the top three bits survive.
///
/// Preemption additionally requires nesting to be enabled: `PFIC_CFGR.NESTCTRL`
/// controls it and `CSR 0xBC1.NEST_LVL` bounds the depth. qingke-rt's V3C
/// startup sets `NEST_LVL = 1`, i.e. one level.
pub(crate) const TIME_DRIVER_PRIORITY: qingke::interrupt::Priority = qingke::interrupt::Priority::P8;

/// Enable global interrupts and make them wake the core from `wfi`.
///
/// On V3C the authoritative register is `gintenr` (CSR 0x800), which `corecfgr`'s
/// `IE_REMAP_EN` maps onto `mstatus.MIE`/`MPIE`. qingke-rt's V3C startup leaves
/// it clear, so an interrupt handler never runs until this is written. WCH's own
/// `startup_CH585.S` does the equivalent with `mstatus = 0x88`.
///
/// `SEVONPEND` (bit 4) matters just as much: QingKe executes `wfi` as `wfe`
/// whenever `PFIC_SCTLR.WFITOWFE` is set, and `qingke-rt` sets that. A `wfe`
/// wakes on an *event*, not on a pending interrupt, so an alarm raised while the
/// executor sleeps inside a critical section (`mstatus.MIE` clear) posts nothing
/// and the core stays asleep until some unrelated wake-up. Measured cost: 8–25 ms
/// per alarm, i.e. a 500 ms `Timer` completing in 510–627 ms. With `SEVONPEND`
/// the same timer is 501.9 ms.
///
/// Shared by both drivers; called from their `init`.
pub(crate) fn enable_interrupts() {
    // `PFIC.sctlr()` also carries `SETEVENT` (bit 5), which the PAC marks as
    // write-only. A `modify()` would read that bit back and write it, so the
    // field is set directly instead: read, OR in just this bit, write.
    let sctlr = pac::PFIC.sctlr().read();
    let mut new = sctlr;
    new.set_sevonpend(true);
    pac::PFIC.sctlr().write_value(new);

    // QingKe V3A has no GINTENR CSR; its global interrupt bit is
    // `mstatus.MIE`. V3C and V4A use CSR 0x800.
    #[cfg(any(ch572, ch573))]
    unsafe {
        core::arch::asm!("csrs mstatus, {}", const 0x8);
    }
    #[cfg(not(any(ch572, ch573)))]
    unsafe {
        qingke::register::gintenr::write(0x08)
    };
}
