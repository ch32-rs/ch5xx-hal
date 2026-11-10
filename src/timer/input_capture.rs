//! Interval capture with mode-dependent timeout/level metadata.

use super::low_level::Timer;
use super::{CaptureEdge, CaptureWord, DmaInstance, Error, Events, Instance, PeriodTicks, TimerPin};
use crate::gpio::{Flex, Pull};
use crate::{dma, Peri};

pub struct InputCapture<'d, T: Instance> {
    pub(crate) inner: Timer<'d, T>,
    _pin: Flex<'d>,
}
impl<'d, T: Instance> InputCapture<'d, T> {
    /// # Panics
    /// Timeout must fit the 25-bit captured interval field. Use
    /// `PeriodTicks::new_capture` to validate it without a constructor panic.
    pub fn new<const REMAP: u8>(
        peri: Peri<'d, T>,
        pin: Peri<'d, impl TimerPin<T, REMAP>>,
        pull: Pull,
        timeout: PeriodTicks,
        edge: CaptureEdge,
    ) -> Self {
        assert!(
            timeout.get() <= super::MAX_CAPTURE_PERIOD,
            "capture timeout exceeds 25-bit interval field"
        );
        let mut pin = Flex::new(pin);
        pin.set_as_input(pull);
        critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });
        let mut inner = Timer::new(peri);
        inner.configure_capture(timeout, edge);
        inner.start();
        Self { inner, _pin: pin }
    }
    pub fn try_capture(&mut self) -> Result<Option<CaptureWord>, Error> {
        if self.inner.events().contains(Events::FIFO_ERROR) {
            return Err(Error::FifoOverrun);
        }
        Ok(unsafe { self.inner.pop_capture() })
    }
    #[cfg(feature = "embassy")]
    pub fn bind_interrupt(
        &mut self,
        irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, super::InterruptHandler<T>>,
    ) {
        self.inner.bind_interrupt(irq);
    }
}

impl<'d, T: DmaInstance> InputCapture<'d, T> {
    pub fn read<'a>(
        &'a mut self,
        buffer: &'static mut [u32],
    ) -> Result<dma::Transfer<'a, 'd, T>, (dma::Error, &'static mut [u32])> {
        if let Err(e) = dma::Descriptor::new(buffer.as_ptr() as usize, buffer.len()) {
            return Err((e, buffer));
        }
        let mode = self.inner.mode;
        let period = self.inner.period;
        unsafe {
            self.inner.configure_raw(mode, period);
        }
        unsafe {
            dma::Transfer::new(
                &mut self.inner,
                buffer,
                dma::Dir::PeripheralToMemory,
                dma::RunMode::Once,
            )
        }
    }
    /// Circular capture does not expose live slices or promise lossless laps.
    pub fn read_circular<'a>(
        &'a mut self,
        buffer: &'static mut [u32],
    ) -> Result<dma::Transfer<'a, 'd, T>, (dma::Error, &'static mut [u32])> {
        if let Err(e) = dma::Descriptor::new(buffer.as_ptr() as usize, buffer.len()) {
            return Err((e, buffer));
        }
        let mode = self.inner.mode;
        let period = self.inner.period;
        unsafe {
            self.inner.configure_raw(mode, period);
        }
        unsafe {
            dma::Transfer::new(
                &mut self.inner,
                buffer,
                dma::Dir::PeripheralToMemory,
                dma::RunMode::Circular,
            )
        }
    }
}
