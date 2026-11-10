//! Single-output FIFO-backed PWM, unlike CH32's independent duty registers.

use super::low_level::Timer;
use super::{DmaInstance, Error, Instance, OutputPolarity, PeriodTicks, Repeat, TimerPin};
use crate::gpio::{Flex, Level, Speed};
use crate::{dma, Peri};

pub struct SimplePwm<'d, T: Instance> {
    pub(crate) inner: Timer<'d, T>,
    _pin: Flex<'d>,
}
impl<'d, T: Instance> SimplePwm<'d, T> {
    pub fn new<const REMAP: u8>(
        peri: Peri<'d, T>,
        pin: Peri<'d, impl TimerPin<T, REMAP>>,
        period: PeriodTicks,
        polarity: OutputPolarity,
        repeat: Repeat,
    ) -> Self {
        let mut pin = Flex::new(pin);
        pin.set_low();
        pin.set_as_output(Speed::High);
        critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });
        let mut inner = Timer::new(peri);
        inner.configure_pwm(period, polarity, repeat);
        Self { inner, _pin: pin }
    }
    /// Restart at a static duty. FIFO updates are not instantaneous CCR writes.
    pub fn set_duty(&mut self, ticks: u32) -> Result<(), Error> {
        if ticks > self.inner.period.get() {
            return Err(Error::InvalidDuty);
        }
        let mode = self.inner.mode;
        let period = self.inner.period;
        unsafe {
            self.inner.configure_raw(mode, period);
            self.inner.push_pwm(ticks)?;
            self.inner.start();
            self.inner.enable_output();
        }
        Ok(())
    }
    pub fn disable(&mut self) {
        self.inner.stop();
    }
    pub fn period(&self) -> PeriodTicks {
        self.inner.period()
    }
    #[cfg(feature = "embassy")]
    pub fn bind_interrupt(
        &mut self,
        irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, super::InterruptHandler<T>>,
    ) {
        self.inner.bind_interrupt(irq);
    }
}

impl<'d, T: DmaInstance> SimplePwm<'d, T> {
    /// Queue a complete waveform, retaining exclusive ownership of its storage.
    pub fn write<'a>(
        &'a mut self,
        buffer: &'static mut [u32],
    ) -> Result<dma::Transfer<'a, 'd, T>, (dma::Error, &'static mut [u32])> {
        if buffer.iter().any(|v| *v > self.inner.period.get()) {
            return Err((dma::Error::InvalidWidth, buffer));
        }
        // Reject invalid storage before resetting an existing static output.
        if let Err(e) = dma::Descriptor::new(buffer.as_ptr() as usize, buffer.len()) {
            return Err((e, buffer));
        }
        // Reset the FIFO and counter left by a previous waveform or static duty.
        let mode = self.inner.mode;
        let period = self.inner.period;
        unsafe {
            self.inner.configure_raw(mode, period);
        }
        unsafe {
            dma::Transfer::new(
                &mut self.inner,
                buffer,
                dma::Dir::MemoryToPeripheral,
                dma::RunMode::Once,
            )
        }
    }
}
impl<T: Instance> Drop for SimplePwm<'_, T> {
    fn drop(&mut self) {
        self.inner.stop();
        self._pin.set_level(Level::Low);
        self._pin.set_as_input(crate::gpio::Pull::None);
    }
}

impl<T: Instance> embedded_hal::pwm::ErrorType for SimplePwm<'_, T> {
    type Error = Error;
}
impl<T: Instance> embedded_hal::pwm::SetDutyCycle for SimplePwm<'_, T> {
    fn max_duty_cycle(&self) -> u16 {
        u16::MAX
    }
    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Error> {
        self.set_duty((self.inner.period.get() as u64 * duty as u64 / u16::MAX as u64) as u32)
    }
}
