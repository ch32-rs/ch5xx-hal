//! Counter frontends. The Embassy timebase reserves its instance separately.

use super::low_level::Timer;
use super::{CaptureEdge, Events, Instance, PeriodTicks, TimerPin};
use crate::gpio::{Flex, Pull};
use crate::Peri;

pub struct Counter<'d, T: Instance> {
    inner: Timer<'d, T>,
}
impl<'d, T: Instance> Counter<'d, T> {
    pub fn new(peri: Peri<'d, T>, period: PeriodTicks) -> Self {
        let mut inner = Timer::new(peri);
        inner.configure_counter(period);
        inner.start();
        Self { inner }
    }
    pub fn count(&self) -> u32 {
        self.inner.count()
    }
    pub fn stop(&mut self) {
        self.inner.stop();
    }
    pub fn restart(&mut self, period: PeriodTicks) {
        self.inner.configure_counter(period);
        self.inner.start();
    }
    /// Consume the sticky period event. Multiple unserviced periods coalesce.
    pub fn elapsed(&mut self) -> bool {
        let elapsed = self.inner.events().contains(Events::CYCLE_END);
        if elapsed {
            self.inner.acknowledge(Events::CYCLE_END);
        }
        elapsed
    }
    #[cfg(feature = "embassy")]
    pub fn bind_interrupt(
        &mut self,
        irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, super::InterruptHandler<T>>,
    ) {
        self.inner.bind_interrupt(irq);
    }
    #[cfg(feature = "embassy")]
    pub async fn wait(
        &mut self,
        irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, super::InterruptHandler<T>>,
    ) {
        self.inner.bind_interrupt(irq);
        self.inner.wait_event(Events::CYCLE_END).await;
        self.inner.acknowledge(Events::CYCLE_END);
    }
}

/// External sampled-edge counter. The raw terminal register uses the vendor's
/// count-mode convention (final count minus two), not a duration in Fsys ticks.
pub struct ExternalCounter<'d, T: Instance> {
    inner: Timer<'d, T>,
    _pin: Flex<'d>,
}
impl<'d, T: Instance> ExternalCounter<'d, T> {
    pub fn new<const REMAP: u8>(
        peri: Peri<'d, T>,
        pin: Peri<'d, impl TimerPin<T, REMAP>>,
        pull: Pull,
        end_register: PeriodTicks,
        edge: CaptureEdge,
    ) -> Self {
        let mut pin = Flex::new(pin);
        pin.set_as_input(pull);
        critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });
        let mut inner = Timer::new(peri);
        unsafe {
            inner.configure_raw(0x11 | ((edge as u8) << 6), end_register);
        }
        inner.start();
        Self { inner, _pin: pin }
    }
    pub fn count(&self) -> u32 {
        self.inner.count()
    }
    pub fn stop(&mut self) {
        self.inner.stop();
    }
}
