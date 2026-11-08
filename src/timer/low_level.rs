//! Instance-owned timer mechanisms. Raw mode changes are unsafe because they
//! can bypass pin routing and DMA ownership. Safe frontends live alongside this
//! module; NFC can coordinate owned timers without a fictitious external pin.

use core::cell::Cell;
use core::marker::PhantomData;

use super::{CaptureEdge, Error, Events, Instance, NfcClockInstance, OutputPolarity, PeriodTicks, Repeat};
use crate::interrupt::typelevel::Interrupt;
use crate::{pac, Peri};

pub struct Timer<'d, T: Instance> {
    _peri: Peri<'d, T>,
    pub(crate) mode: u8,
    pub(crate) period: PeriodTicks,
    pub(crate) repeat: Repeat,
    _not_sync: PhantomData<Cell<()>>,
}

impl<'d, T: Instance> Timer<'d, T> {
    pub fn new(peri: Peri<'d, T>) -> Self {
        let mut this = Self {
            _peri: peri,
            mode: 0,
            period: PeriodTicks::new(2).unwrap(),
            repeat: Repeat::Once,
            _not_sync: PhantomData,
        };
        this.reset();
        this
    }
    #[inline]
    pub(crate) fn regs(&self) -> pac::tmr::TmrFull {
        T::regs()
    }

    /// Stop all requests before reset; ALL_CLEAR is a held level.
    pub fn reset(&mut self) {
        self.stop();
        self.regs().ctrl_mod().write(|w| w.0 = 2);
        self.regs().int_flag().write(|w| w.0 = 31);
        T::Interrupt::unpend();
        #[cfg(feature = "embassy")]
        T::state().events.store(0, crate::atomic::Ordering::Relaxed);
    }
    /// Quiesce the peripheral's request sources, then order SRAM accesses.
    pub fn stop(&mut self) {
        self.regs().inter_en().write(|w| w.0 = 0);
        self.regs().ctrl_mod().modify(|w| w.0 &= !0x0c);
        if T::HAS_DMA {
            self.regs().ctrl_dma().write(|w| w.0 = 0);
            crate::dma::fence();
            let _ = self.regs().ctrl_dma().read();
        }
        // Serialize prior DMA bus activity before CPU access to its storage.
        crate::dma::fence();
    }
    pub fn configure_counter(&mut self, period: PeriodTicks) {
        self.reset();
        self.mode = 0;
        self.set_period_stopped(period);
        self.regs().ctrl_mod().write(|w| w.0 = self.mode);
    }
    pub(crate) fn configure_capture(&mut self, period: PeriodTicks, edge: CaptureEdge) {
        self.reset();
        self.mode = 1 | ((edge as u8) << 6);
        self.set_period_stopped(period);
        self.regs().ctrl_mod().write(|w| w.0 = self.mode);
    }
    pub(crate) fn configure_pwm(&mut self, period: PeriodTicks, polarity: OutputPolarity, repeat: Repeat) {
        self.reset();
        self.repeat = repeat;
        self.mode = ((repeat as u8) << 6) | if polarity == OutputPolarity::ActiveLow { 16 } else { 0 };
        self.set_period_stopped(period);
        self.regs().ctrl_mod().write(|w| w.0 = self.mode);
    }
    fn set_period_stopped(&mut self, period: PeriodTicks) {
        self.period = period;
        self.regs().cnt_end().write(|w| w.set_cnt_end(period.get()));
    }
    /// Start the configured mode. This does not enable an external PWM output.
    pub fn start(&mut self) {
        self.regs().ctrl_mod().write(|w| w.0 = self.mode | 4);
    }
    pub fn count(&self) -> u32 {
        self.regs().count().read().count()
    }
    pub fn period(&self) -> PeriodTicks {
        self.period
    }
    pub fn events(&self) -> Events {
        let bits = self.regs().int_flag().read().0;
        #[cfg(feature = "embassy")]
        let bits = bits | T::state().events.load(crate::atomic::Ordering::Relaxed);
        Events(bits & 31)
    }
    /// Acknowledge ordinary events. DMA_END requires the DMA stop/rearm path.
    pub fn acknowledge(&mut self, events: Events) {
        let bits = events.bits() & !8;
        self.regs().int_flag().write(|w| w.0 = bits);
        #[cfg(feature = "embassy")]
        T::state().events.fetch_and(!bits, crate::atomic::Ordering::Relaxed);
    }
    /// Enable event-driven waits; the binding installs this timer's handler.
    #[cfg(feature = "embassy")]
    pub fn bind_interrupt(
        &mut self,
        _irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, super::InterruptHandler<T>>,
    ) {
        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };
    }
    #[cfg(feature = "embassy")]
    pub(crate) async fn wait_event(&mut self, events: Events) -> Events {
        use core::task::Poll;
        let r = self.regs();
        let mask_on_drop = crate::internal::drop::OnDrop::new(|| {
            r.inter_en().modify(|w| w.0 &= !events.bits());
        });
        let result = core::future::poll_fn(|cx| {
            T::state().waker.register(cx.waker());
            self.regs().inter_en().modify(|w| w.0 |= events.bits());
            let pending = self.events();
            if pending.bits() & events.bits() != 0 {
                self.regs().inter_en().modify(|w| w.0 &= !events.bits());
                Poll::Ready(pending)
            } else {
                Poll::Pending
            }
        })
        .await;
        mask_on_drop.defuse();
        result
    }
    /// Access mode-specific mechanisms for an internal signal route.
    ///
    /// # Safety
    /// Own the signal route/pins, ensure no DMA references survive mode changes,
    /// and use only documented CH585 mode bits. The caller owns output safety.
    pub unsafe fn configure_raw(&mut self, mode: u8, period: PeriodTicks) {
        self.reset();
        self.mode = mode & !0x0e;
        self.set_period_stopped(period);
        self.regs().ctrl_mod().write(|w| w.0 = self.mode);
    }
    /// # Safety
    /// The caller owns the output route and has configured PWM mode.
    pub unsafe fn enable_output(&mut self) {
        self.regs().ctrl_mod().modify(|w| w.0 |= 8);
    }
    /// # Safety
    /// Must be stopped or in PWM mode with exclusive FIFO producer ownership.
    pub unsafe fn push_pwm(&mut self, ticks: u32) -> Result<(), Error> {
        if ticks > self.period.get() {
            return Err(Error::InvalidDuty);
        }
        self.regs().fifo().write(|w| w.set_fifo(ticks));
        Ok(())
    }
    /// # Safety
    /// Must be in capture mode with DMA disabled and exclusive FIFO consumer.
    pub unsafe fn pop_capture(&mut self) -> Option<super::CaptureWord> {
        if self.regs().fifo_count().read().fifo_count() == 0 {
            None
        } else {
            Some(super::CaptureWord(self.regs().fifo().read().fifo()))
        }
    }
}
impl<T: NfcClockInstance> Timer<'_, T> {
    /// Select the SDK's dedicated 13.56 MHz mode while stopped.
    pub fn set_nfc_clock(&mut self) -> Result<(), Error> {
        if crate::rcc::clocks().sysclk.0 < 42_000_000 {
            return Err(Error::ClockTooSlow);
        }
        self.stop();
        self.mode |= 0x20;
        self.regs().ctrl_mod().write(|w| w.0 = self.mode);
        Ok(())
    }
}
impl<T: Instance> Drop for Timer<'_, T> {
    fn drop(&mut self) {
        self.reset();
        T::Interrupt::disable();
    }
}
