//! PWM4 to PWM11.
//!
//! One block clocks every channel from `Fsys`. `CLOCK_DIV` divides that clock,
//! and the cycle length is shared too: 256, 255, 128, 127, 64 or 63 clocks. The
//! output frequency is `Fsys / divisor / cycle`. A divisor of 0 is rejected.
//!
//! Each channel holds its own duty in one byte. The 256-clock cycle still has an
//! 8-bit duty register, so the largest duty is 255. Duty 0 leaves the pin at its
//! idle level.
//!
//! `PIN_PWMX` remaps PWM4, PWM5, PWM7, PWM8 and PWM9 together. PWM6, PWM10 and
//! PWM11 have no alternate pin. Mixing a remapped pin with a default one flips
//! the shared bit, so every channel has to agree. The block raises `PWMX_SPI1`,
//! the same line as SPI1; this driver does not claim the interrupt.
//!
//! The register layout is the CH585 one (CH581, CH584, CH585). CH595 selects the
//! same registers but its family file lists no pins. 16-bit cycle mode is not
//! implemented.

use core::convert::Infallible;
use core::marker::PhantomData;

use embassy_hal_internal::PeripheralType;
use embedded_hal::pwm::{ErrorType, SetDutyCycle};

use crate::gpio::{Flex, Pin, Speed};
use crate::{pac, peripherals, Peri};

/// Why a constructor refused the configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The divisor is zero, which is not a clock divider.
    Divisor,
}

/// Shared cycle length, in clocks of the divided PWM clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cycle {
    /// 256 clocks. The duty register still stops at 255.
    Clocks256,
    /// 255 clocks.
    Clocks255,
    /// 128 clocks.
    Clocks128,
    /// 127 clocks.
    Clocks127,
    /// 64 clocks.
    Clocks64,
    /// 63 clocks.
    Clocks63,
}

impl Cycle {
    /// Clocks in one period.
    pub const fn clocks(self) -> u16 {
        match self {
            Cycle::Clocks256 => 256,
            Cycle::Clocks255 => 255,
            Cycle::Clocks128 => 128,
            Cycle::Clocks127 => 127,
            Cycle::Clocks64 => 64,
            Cycle::Clocks63 => 63,
        }
    }

    /// Largest value the duty register can hold for this cycle.
    const fn max_duty(self) -> u16 {
        match self {
            Cycle::Clocks256 => 255,
            other => other.clocks(),
        }
    }

    /// `CONFIG.CYC_MOD` and `CONFIG.CYCLE_SEL`, from the vendor cycle setup.
    fn fields(self) -> (u8, bool) {
        match self {
            Cycle::Clocks256 => (0, false),
            Cycle::Clocks255 => (0, true),
            Cycle::Clocks128 => (1, false),
            Cycle::Clocks127 => (1, true),
            Cycle::Clocks64 => (2, false),
            Cycle::Clocks63 => (2, true),
        }
    }
}

/// Which level the pulse takes. The pin idles at the other level.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Polarity {
    /// Idle low, high during the duty.
    ActiveHigh,
    /// Idle high, low during the duty.
    ActiveLow,
}

/// Shared PWM clock and cycle.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Config {
    /// `CLOCK_DIV`. The channel clock is `Fsys / divisor`.
    pub divisor: u8,
    /// Period shared by every channel.
    pub cycle: Cycle,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            divisor: 1,
            cycle: Cycle::Clocks256,
        }
    }
}

impl Config {
    /// `divisor` and `cycle`. A divisor of zero is rejected by [`Pwm::new`].
    pub const fn new(divisor: u8, cycle: Cycle) -> Self {
        Self { divisor, cycle }
    }
}

/// The PWM4-PWM11 block.
pub struct Pwm<'d, T: Instance> {
    cycle: Cycle,
    _phantom: PhantomData<&'d mut T>,
}

impl<'d, T: Instance> Pwm<'d, T> {
    /// Program the shared clock and cycle. No channel drives a pin yet.
    pub fn new(_peri: Peri<'d, T>, config: Config) -> Result<Self, ConfigError> {
        if config.divisor == 0 {
            return Err(ConfigError::Divisor);
        }
        let regs = T::regs();
        regs.clock_div().write(|w| w.set_clock_div(config.divisor));
        let (width, short) = config.cycle.fields();
        // Leave the stagger-enable bits alone. They are not part of the cycle.
        regs.config().modify(|w| {
            w.set_cyc_mod(width);
            w.set_cycle_sel(short);
        });
        Ok(Self {
            cycle: config.cycle,
            _phantom: PhantomData,
        })
    }

    /// Drive `pin` from its channel.
    ///
    /// The returned output borrows the block, so the clock cannot be rebuilt
    /// while the pin is live. Dropping it releases the pin and clears the
    /// channel's output enable.
    pub fn channel<'a, P: PwmPin<T>>(&'a mut self, pin: Peri<'a, P>, polarity: Polarity) -> Channel<'a, T> {
        if let Some(remapped) = P::REMAP {
            critical_section::with(|_| unsafe { T::set_remap(remapped) });
        }
        let mut pin = Flex::new(pin.into());
        match polarity {
            Polarity::ActiveHigh => pin.set_low(),
            Polarity::ActiveLow => pin.set_high(),
        }
        pin.set_as_output(Speed::High);
        set_polarity::<T>(P::INDEX, polarity);
        set_duty::<T>(P::INDEX, 0);
        set_output::<T>(P::INDEX, true);
        Channel {
            index: P::INDEX,
            max_duty: self.cycle.max_duty(),
            pin,
            _instance: PhantomData,
        }
    }
}

impl<'d, T: Instance> Drop for Pwm<'d, T> {
    fn drop(&mut self) {
        T::regs().out_en().write(|w| {
            w.set_pwm4_out_en(false);
            w.set_pwm5_out_en(false);
            w.set_pwm6_out_en(false);
            w.set_pwm7_out_en(false);
            w.set_pwm8_out_en(false);
            w.set_pwm9_out_en(false);
            w.set_pwm10_out_en(false);
            w.set_pwm11_out_en(false);
        });
    }
}

/// One channel output.
pub struct Channel<'a, T: Instance> {
    index: u8,
    max_duty: u16,
    pin: Flex<'a>,
    _instance: PhantomData<&'a mut T>,
}

impl<'a, T: Instance> Channel<'a, T> {
    /// Duty in clocks, clamped to the cycle.
    pub fn set_duty(&mut self, duty: u16) {
        let _ = self.set_duty_cycle(duty);
    }
}

impl<'a, T: Instance> Drop for Channel<'a, T> {
    fn drop(&mut self) {
        set_output::<T>(self.index, false);
        self.pin.set_as_input(crate::gpio::Pull::Up);
    }
}

impl<'a, T: Instance> ErrorType for Channel<'a, T> {
    type Error = Infallible;
}

impl<'a, T: Instance> SetDutyCycle for Channel<'a, T> {
    fn max_duty_cycle(&self) -> u16 {
        self.max_duty
    }

    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Self::Error> {
        set_duty::<T>(self.index, duty.min(self.max_duty) as u8);
        Ok(())
    }
}

fn set_duty<T: Instance>(index: u8, duty: u8) {
    let regs = T::regs();
    match index {
        0 => regs.pwm4_7_data().modify(|w| w.set_pwm4_data__pwm4_data(duty)),
        1 => regs.pwm4_7_data().modify(|w| w.set_pwm5_data__pwm4_data(duty)),
        2 => regs.pwm4_7_data().modify(|w| w.set_pwm6_data__pwm5_data(duty)),
        3 => regs.pwm4_7_data().modify(|w| w.set_pwm7_data__pwm5_data(duty)),
        4 => regs.pwm8_11_data().modify(|w| w.set_pwm8_data__pwm6_data(duty)),
        5 => regs.pwm8_11_data().modify(|w| w.set_pwm9_data__pwm6_data(duty)),
        6 => regs.pwm8_11_data().modify(|w| w.set_pwm10_data__pwm7_data(duty)),
        _ => regs.pwm8_11_data().modify(|w| w.set_pwm11_data__pwm7_data(duty)),
    }
}

fn set_polarity<T: Instance>(index: u8, polarity: Polarity) {
    let active_low = polarity == Polarity::ActiveLow;
    let regs = T::regs();
    regs.polar().modify(|w| match index {
        0 => w.set_pwm4_polar(active_low),
        1 => w.set_pwm5_polar(active_low),
        2 => w.set_pwm6_polar(active_low),
        3 => w.set_pwm7_polar(active_low),
        4 => w.set_pwm8_polar(active_low),
        5 => w.set_pwm9_polar(active_low),
        6 => w.set_pwm10_polar(active_low),
        _ => w.set_pwm11_polar(active_low),
    });
}

fn set_output<T: Instance>(index: u8, enabled: bool) {
    let regs = T::regs();
    regs.out_en().modify(|w| match index {
        0 => w.set_pwm4_out_en(enabled),
        1 => w.set_pwm5_out_en(enabled),
        2 => w.set_pwm6_out_en(enabled),
        3 => w.set_pwm7_out_en(enabled),
        4 => w.set_pwm8_out_en(enabled),
        5 => w.set_pwm9_out_en(enabled),
        6 => w.set_pwm10_out_en(enabled),
        _ => w.set_pwm11_out_en(enabled),
    });
}

// --- instance and pins -----------------------------------------------------

mod sealed {
    use super::pac;

    pub trait Instance {
        fn regs() -> pac::pwmx::Pwmx;

        /// Set `PIN_ALTERNATE.PIN_PWMX`.
        ///
        /// # Safety
        ///
        /// The register is shared, so the caller holds a critical section.
        unsafe fn set_remap(remapped: bool);
    }
}

/// The PWM4-PWM11 block.
#[allow(private_bounds)]
pub trait Instance: sealed::Instance + PeripheralType {}

impl sealed::Instance for peripherals::PWMX {
    fn regs() -> pac::pwmx::Pwmx {
        pac::PWMX
    }

    unsafe fn set_remap(remapped: bool) {
        pac::SYS.pin_alternate().modify(|w| w.set_pin_pwmx(remapped));
    }
}

impl Instance for peripherals::PWMX {}

/// A pin that can carry one PWM channel.
///
/// `REMAP` is `Some(true)` for the alternate pin, `Some(false)` for the default
/// pin, and `None` when the channel has no alternate and the shared bit must be
/// left alone.
pub trait PwmPin<T: Instance>: Pin {
    /// Channel index, PWM4 is 0.
    const INDEX: u8;
    /// What to write to `PIN_PWMX`, or `None` to leave it.
    const REMAP: Option<bool>;
}

macro_rules! impl_pin {
    ($pin:ident, $index:literal, $remap:expr) => {
        impl PwmPin<peripherals::PWMX> for peripherals::$pin {
            const INDEX: u8 = $index;
            const REMAP: Option<bool> = $remap;
        }
    };
}

#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PA12, 0, Some(false));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PA6, 0, Some(true));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PA13, 1, Some(false));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PA7, 1, Some(true));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB0, 2, None);
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB4, 3, Some(false));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB1, 3, Some(true));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB6, 4, Some(false));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB2, 4, Some(true));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB7, 5, Some(false));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB3, 5, Some(true));
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB14, 6, None);
#[cfg(any(ch581, ch584, ch585))]
impl_pin!(PB23, 7, None);
