//! Embassy time driver on the CH573 SysTick counter.
//!
//! # Why CH573 needs its own driver
//!
//! CH573's SysTick is the V3A core timer, and it does not behave like the
//! SysTick on the other CH5xx dies. The datasheet (CH573/CH571, section 3.4.2)
//! and the vendor `core_riscv.h` agree on this model:
//!
//! - `CNT` is a **64-bit descending** counter (`CNTL`/`CNTH`).
//! - The interrupt flag (`CNTFG.CNTIF`) is set when `CNT` reaches 0.
//! - `CMP` is a *reload* value, not a compare value.
//! - `CTLR.STRELOAD` is a write-1 **one-shot** load of `CMP` into `CNT`, not an
//!   automatic reload. `CTLR` has no `MODE` or `STRE` bit.
//!
//! The shared [`super::time_driver_systick`] driver assumes an up-counter that
//! reloads at `CMPLR` in hardware. That model is wrong here, so this module
//! implements the down-counter/one-shot-load model instead.
//!
//! # Timebase
//!
//! Each arm loads `CNT` with the period and records two values under a
//! critical section: `base` (the tick time at the load) and `period` (the
//! loaded value). The current tick is
//!
//! ```text
//! now = base + period - CNT   (wrapping, 64-bit)
//! ```
//!
//! which stays correct before *and* after the counter crosses zero: after the
//! crossing the hardware wraps `CNT` to `2^64 - 1`, and the wrapping
//! subtraction turns into `period + ticks_since_zero`.
//!
//! The zero-crossing interrupt advances the anchor to the observed `now` and
//! reloads the counter for the next deadline, mirroring
//! [`super::reload_timer`]'s rule that the hardware decides the period
//! boundary. A deadline farther than [`MAX_PERIOD`] is armed in several
//! periods; when the queue is empty the same happens with no waker due.
//!
//! # No atomic read-modify-write
//!
//! As everywhere else in this HAL, `fetch_add`/`compare_exchange` must not be
//! used on the V3A/V3C cores. The shared state uses plain loads/stores or a
//! [`critical_section`].

use core::cell::{Cell, RefCell};

use critical_section::CriticalSection;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time_queue_utils::Queue;

use crate::atomic::{AtomicU32, Ordering};
use crate::pac::SYSTICK;

/// Longest span programmed into one reload period.
///
/// At the 60 MHz CH573 maximum this is about 5.8 hours. It only bounds how far
/// the software anchor can drift from the hardware counter, and with an idle
/// queue it sets the re-arm interrupt rate.
const MAX_PERIOD: u64 = 1 << 40;

/// Shortest period, in clocks. Covers the register writes between arming and
/// the counter passing the end of the period.
const MIN_PERIOD: u64 = 64;

/// Read `CNT` as one 64-bit value.
///
/// The counter is 32 bits wide on the bus. A borrow between the two reads
/// changes `CNTH`, so compare it around the low-half read and retry.
#[inline]
fn read_count() -> u64 {
    loop {
        let high = SYSTICK.cnth().read().cnth();
        let low = SYSTICK.cntl().read().cntl();
        if SYSTICK.cnth().read().cnth() == high {
            return (high as u64) << 32 | low as u64;
        }
    }
}

/// Clear the zero-crossing flag. `CNTFG.CNTIF` is RW0: write 0 to clear.
#[inline]
fn clear_count_flag() {
    SYSTICK.cntfg().modify(|w| w.set_cntif(false));
}

/// Load `period` into the counter through the one-shot reload path.
///
/// `CMP` is only meaningful as the source of that load; the interrupt is
/// raised when `CNT` reaches zero, not when it equals `CMP`.
#[inline]
fn load_period(period: u64) {
    SYSTICK.cmplr().write(|w| w.set_cmpl(period as u32));
    SYSTICK.cmphr().write(|w| w.set_cmph((period >> 32) as u32));
    SYSTICK.ctlr().modify(|w| w.set_streload(true));
}

struct State {
    /// Configured nominal counter ticks per second.
    hz: AtomicU32,
    /// Tick count at the instant the counter was last loaded.
    base: Mutex<CriticalSectionRawMutex, Cell<u64>>,
    /// Value last loaded into the counter.
    period: Mutex<CriticalSectionRawMutex, Cell<u64>>,
    queue: Mutex<CriticalSectionRawMutex, RefCell<Queue>>,
}

impl State {
    const fn new() -> Self {
        Self {
            hz: AtomicU32::new(1), // replaced by `reset` before use
            base: Mutex::new(Cell::new(0)),
            period: Mutex::new(Cell::new(0)),
            queue: Mutex::new(RefCell::new(Queue::new())),
        }
    }

    /// Current 64-bit tick count.
    ///
    /// Interrupts must be disabled: `read_count` and the two anchor cells have
    /// to be sampled as one coherent picture, or a reload in between would
    /// combine a new anchor with an old counter.
    #[inline]
    fn ticks_locked(&self, cs: CriticalSection) -> u64 {
        let period = self.period.borrow(cs).get();
        let base = self.base.borrow(cs).get();
        base.wrapping_add(period.wrapping_sub(read_count()))
    }

    #[inline]
    fn ticks(&self) -> u64 {
        critical_section::with(|cs| self.ticks_locked(cs))
    }

    /// Counter ticks → embassy ticks, rounding down.
    #[inline]
    fn ticks_to_time(&self, ticks: u64) -> u64 {
        let hz = self.hz.load(Ordering::Relaxed) as u64;
        ticks / hz * embassy_time_driver::TICK_HZ + ticks % hz * embassy_time_driver::TICK_HZ / hz
    }

    /// Embassy ticks → counter ticks, rounding up.
    #[inline]
    fn time_to_ticks(&self, time: u64) -> u64 {
        let hz = self.hz.load(Ordering::Relaxed) as u64;
        let whole = (time / embassy_time_driver::TICK_HZ).saturating_mul(hz);
        let frac = (time % embassy_time_driver::TICK_HZ * hz).div_ceil(embassy_time_driver::TICK_HZ);
        whole.saturating_add(frac)
    }

    /// Load the next period for absolute counter tick `at`.
    ///
    /// `now` is the anchor sampled by the caller. It must be read before any
    /// state changes, because re-reading after the counter is re-origined
    /// would count the elapsed period twice. `at == u64::MAX` (empty queue)
    /// selects `MAX_PERIOD`.
    fn arm(&self, cs: CriticalSection, now: u64, at: u64) {
        // The few clocks between the caller's read and this one-shot load are
        // the only drift introduced per re-arm.
        let period = at.saturating_sub(now).min(MAX_PERIOD).max(MIN_PERIOD);

        clear_count_flag();
        load_period(period);

        self.base.borrow(cs).set(now);
        self.period.borrow(cs).set(period);
    }

    /// Wake everything due and arm the next deadline.
    fn trigger_alarm(&self, cs: CriticalSection) {
        let now = self.ticks_locked(cs);
        self.trigger_alarm_at(cs, now);
    }

    /// [`Self::trigger_alarm`] for a caller that already sampled the tick
    /// count, such as the zero-crossing interrupt.
    fn trigger_alarm_at(&self, cs: CriticalSection, now: u64) {
        let next = self.queue.borrow(cs).borrow_mut().next_expiration(now);
        self.arm(cs, now, next);
    }

    /// Record the configured rate and reset the timebase to zero.
    fn reset(&self, cs: CriticalSection, hz: u32) {
        self.hz.store(hz, Ordering::Relaxed);
        self.base.borrow(cs).set(0);
        self.period.borrow(cs).set(0);
    }
}

struct Driver {
    state: State,
}

embassy_time_driver::time_driver_impl!(static DRIVER: Driver = Driver {
    state: State::new(),
});

impl embassy_time_driver::Driver for Driver {
    fn now(&self) -> u64 {
        self.state.ticks_to_time(self.state.ticks())
    }

    fn schedule_wake(&self, at: u64, waker: &core::task::Waker) {
        let ticks = self.state.time_to_ticks(at);
        critical_section::with(|cs| {
            self.state.queue.borrow(cs).borrow_mut().schedule_wake(ticks, waker);
            self.state.trigger_alarm(cs);
        });
    }
}

/// SysTick's core-interrupt vector (core interrupt 12).
#[qingke_rt::interrupt(core)]
fn SysTick() {
    if !SYSTICK.cntfg().read().cntif() {
        // Not ours. Leave the flag alone; nothing else uses this counter.
        return;
    }
    clear_count_flag();

    critical_section::with(|cs| {
        // The counter has crossed zero. Sample the true time once, then run
        // the queue and load the next deadline. `arm` moves the anchor to
        // this value, so the interrupt latency is preserved instead of
        // discarded.
        let now = DRIVER.state.ticks_locked(cs);
        DRIVER.state.trigger_alarm_at(cs, now);
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

    // Stop before changing the count or reload value.
    stk.ctlr().modify(|w| w.set_ste(false));
    clear_count_flag();
    stk.cntl().write(|w| w.set_cntl(0));
    stk.cnth().write(|w| w.set_cnth(0));
    stk.ctlr().modify(|w| {
        w.set_stclk(true); // HCLK, not HCLK/8
        w.set_stie(false);
    });

    critical_section::with(|cs| {
        DRIVER.state.reset(cs, hz);
        // Starts near the maximum possible counter value, so the first
        // zero-crossing is a full period away.
        DRIVER.state.arm(cs, 0, u64::MAX);
    });

    stk.ctlr().modify(|w| {
        w.set_stie(true); // interrupt at the zero crossing
        w.set_ste(true);
    });

    // SysTick is a *core* interrupt, so it is not in the external interrupt
    // table the generated `interrupt::typelevel` types come from. qingke's
    // PFIC API is the only route; the priority still goes through `Priority`.
    let irq = qingke_rt::CoreInterrupt::SysTick as u8;
    unsafe {
        qingke::pfic::set_priority(irq, super::TIME_DRIVER_PRIORITY as u8);
        qingke::pfic::enable_interrupt(irq);
    }

    super::enable_interrupts();
}
