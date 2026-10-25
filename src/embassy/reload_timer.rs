//! State shared by the SysTick and TMR time drivers.
//!
//! Both CH5xx counters are "reload at the end value" timers, so the 64-bit
//! timebase is built the same way: the hardware advances `high` by exactly the
//! period that was programmed, and `now()` is `high + counter`. Only the
//! register access differs, which each driver supplies through [`ReloadTimer`].
//!
//! See [`super::time_driver_systick`] for why the reload model matters.

use core::cell::{Cell, RefCell};

use critical_section::CriticalSection;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time_queue_utils::Queue;

use crate::atomic::Ordering;

/// Longest span between interrupt periods, as a fraction of the counter's own
/// range.
///
/// Purely an upper bound on how far `high` can be from the hardware counter; a
/// wake scheduled further out is split into several periods. Half the range
/// keeps the end value far from the reload point, so `MIN_PERIOD` can never
/// push the period past the counter's width.
const MAX_PERIOD_FRACTION_SHIFT: u32 = 1;

/// Minimum lead between the counter and a newly written end value.
///
/// Reading the counter and writing the end value are separate bus accesses; if
/// the counter passed the new value in between, the match is lost until the
/// counter wraps all the way round. A few dozen ticks of margin makes that
/// impossible.
pub(crate) const MIN_PERIOD: u32 = 64;

/// A counter that reloads at a programmable end value and raises an interrupt
/// every time it does.
pub(crate) trait ReloadTimer {
    /// Width mask of the counter, e.g. `0xFFFF_FFFF` for SysTick, `0x03FF_FFFF`
    /// for TMR.
    const MASK: u32;

    /// Longest period that may be programmed into the end register.
    const MAX_PERIOD: u32 = Self::MASK >> MAX_PERIOD_FRACTION_SHIFT;

    /// Current counter value.
    fn count(&self) -> u32;

    /// Program the end value (the counter reloads when it reaches this) and
    /// clear any pending interrupt flag.
    fn set_end(&self, end: u32);

    /// Clear the period-end interrupt flag.
    fn clear_flag(&self);
}

/// The parts of a time driver that do not depend on which counter is used.
pub(crate) struct ReloadState {
    /// Counter ticks per second, as measured at init.
    pub(crate) hz: crate::atomic::AtomicU32,
    /// Counter value at the start of the current hardware period.
    ///
    /// Written only by the interrupt; read together with the counter under a
    /// critical section so the pair is coherent (a 64-bit load can tear).
    pub(crate) high: Mutex<CriticalSectionRawMutex, Cell<u64>>,
    /// Length of the period currently programmed into the end register.
    ///
    /// The interrupt advances `high` by this exact amount.
    pub(crate) period: Mutex<CriticalSectionRawMutex, Cell<u32>>,
    pub(crate) queue: Mutex<CriticalSectionRawMutex, RefCell<Queue>>,
}

impl ReloadState {
    pub(crate) const fn new<T: ReloadTimer>() -> Self {
        Self {
            hz: crate::atomic::AtomicU32::new(1), // replaced by `init` before use
            high: Mutex::new(Cell::new(0)),
            period: Mutex::new(Cell::new(T::MAX_PERIOD)),
            queue: Mutex::new(RefCell::new(Queue::new())),
        }
    }

    /// 64-bit counter tick count, with the pair read atomically.
    ///
    /// Interrupts must be disabled: the period interrupt writes `high` and
    /// reloads the counter together, and reading across that boundary would
    /// combine a new `high` with an old (or reloaded) counter.
    #[inline]
    pub(crate) fn ticks_locked<T: ReloadTimer>(&self, cs: CriticalSection, t: &T) -> u64 {
        self.high.borrow(cs).get() + (t.count() & T::MASK) as u64
    }

    #[inline]
    pub(crate) fn ticks<T: ReloadTimer>(&self, t: &T) -> u64 {
        critical_section::with(|cs| self.ticks_locked(cs, t))
    }

    /// Counter ticks → embassy ticks, rounding down.
    ///
    /// Rounds down so `now()` never reports a time it has not reached yet.
    #[inline]
    pub(crate) fn ticks_to_time(&self, ticks: u64) -> u64 {
        let hz = self.hz.load(Ordering::Relaxed) as u64;
        ticks / hz * embassy_time_driver::TICK_HZ + ticks % hz * embassy_time_driver::TICK_HZ / hz
    }

    /// Embassy ticks → counter ticks, rounding up.
    ///
    /// Rounds up so an alarm is never armed before the instant it was asked
    /// for, which would otherwise cost an extra interrupt. Saturates instead of
    /// wrapping: a deadline so far out that it overflows is effectively "never".
    #[inline]
    pub(crate) fn time_to_ticks(&self, time: u64) -> u64 {
        let hz = self.hz.load(Ordering::Relaxed) as u64;
        let whole = (time / embassy_time_driver::TICK_HZ).saturating_mul(hz);
        let frac = (time % embassy_time_driver::TICK_HZ * hz).div_ceil(embassy_time_driver::TICK_HZ);
        whole.saturating_add(frac)
    }

    /// Program the next interrupt for absolute counter tick `at`.
    ///
    /// `at` is the queue's next expiration, or [`u64::MAX`] when it is empty.
    pub(crate) fn arm<T: ReloadTimer>(&self, cs: CriticalSection, t: &T, at: u64) {
        let base = self.high.borrow(cs).get();
        let mut period = (at - base).min(T::MAX_PERIOD as u64) as u32;

        // Re-read the counter here: waking the timers between computing `at`
        // and writing the end value takes long enough to move it.
        let cnt = t.count() & T::MASK;
        period = period.max(cnt.saturating_add(MIN_PERIOD)).min(T::MASK);

        t.clear_flag();
        t.set_end(period - 1);
        self.period.borrow(cs).set(period);
    }

    /// Run the queue at the current time and program the next interrupt.
    pub(crate) fn trigger_alarm<T: ReloadTimer>(&self, cs: CriticalSection, t: &T) {
        let now = self.ticks_locked(cs, t);
        let next = self.queue.borrow(cs).borrow_mut().next_expiration(now);
        self.arm(cs, t, next);
    }

    /// Advance across a completed period and re-arm. Called from the interrupt.
    pub(crate) fn on_period<T: ReloadTimer>(&self, cs: CriticalSection, t: &T) {
        // The hardware reloaded the counter at the end value, so the high part
        // advances by exactly the programmed period regardless of how late the
        // interrupt was taken.
        let period = self.period.borrow(cs).get();
        let high = self.high.borrow(cs).get() + period as u64;
        self.high.borrow(cs).set(high);

        self.trigger_alarm(cs, t);
    }

    /// Add a waker for `at` and re-arm.
    pub(crate) fn schedule_wake<T: ReloadTimer>(&self, t: &T, at: u64, waker: &core::task::Waker) {
        // The queue works in counter ticks; convert once, at the boundary.
        let ticks = self.time_to_ticks(at);
        critical_section::with(|cs| {
            self.queue.borrow(cs).borrow_mut().schedule_wake(ticks, waker);

            // Re-arm unconditionally. If the deadline is unchanged this writes
            // the same end value; if it is earlier, the interrupt has to be
            // pulled in. `trigger_alarm` also drops anything already due, which
            // is what `Timer` relies on when it re-registers a waker.
            self.trigger_alarm(cs, t);
        });
    }

    /// Record the measured rate and reset the timebase to zero.
    pub(crate) fn reset<T: ReloadTimer>(&self, cs: CriticalSection, hz: u32) {
        self.hz.store(hz, Ordering::Relaxed);
        self.high.borrow(cs).set(0);
        self.period.borrow(cs).set(T::MAX_PERIOD);
    }
}
