//! GPIO
//!
//! Unlike the CH32 families there is **one** GPIO block, `pac::GPIO`, and each
//! port is a group of registers inside it (`PA_DIR`/`PB_DIR`, `PA_OUT`/`PB_OUT`,
//! …) rather than a peripheral of its own. A pin is therefore identified by
//! (port, bit) and the driver dispatches on the port letter.
//!
//! Per-port register set (CH585, offsets from `0x4000_1000`): interrupt enable,
//! mode and flag for PA/PB, then `DIR`, `PIN`, `OUT`, `CLR`, `PU`, `PD_DRV` and
//! `SET`. `CLR`/`SET` are write-1-to-act, so they are the atomic way to change
//! one pin.
//!
//! Power-on state: every pin is a floating input.

use core::convert::Infallible;

use critical_section::CriticalSection;

use crate::{impl_peripheral, pac, peripherals, Peri};

/// Output drive strength.
///
/// CH5xx folds the drive strength into `Px_PD_DRV`: for an output, a 0 selects
/// 5 mA and a 1 selects 20 mA.
#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
pub enum Speed {
    /// 5 mA
    #[default]
    Low,
    /// 20 mA
    High,
}

/// Pull setting for an input.
#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
pub enum Pull {
    #[default]
    None,
    Up,
    Down,
}

/// Logic level.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Level {
    Low,
    High,
}

impl From<bool> for Level {
    fn from(val: bool) -> Self {
        match val {
            true => Self::High,
            false => Self::Low,
        }
    }
}

impl From<Level> for bool {
    fn from(level: Level) -> bool {
        match level {
            Level::Low => false,
            Level::High => true,
        }
    }
}

/// Which physical port a pin lives on.
///
/// The CH5xx exposes `PA` and `PB` as register groups, so this is the
/// discriminator the driver switches on.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Port {
    A,
    B,
}

/// Type-erased GPIO pin.
pub struct AnyPin {
    port: Port,
    pin: u8,
}

impl AnyPin {
    /// Create a pin from its port and bit number.
    ///
    /// # Safety
    ///
    /// The caller must ensure the (port, bit) pair is not used through any
    /// other handle at the same time.
    #[inline]
    pub unsafe fn steal(port: Port, pin: u8) -> Self {
        Self { port, pin }
    }
}

impl_peripheral!(AnyPin);

#[allow(private_bounds)]
pub trait Pin: crate::PeripheralType + Into<AnyPin> + SealedPin + Sized + 'static {
    /// Bit number within the port.
    #[inline]
    fn pin(&self) -> u8 {
        self._pin()
    }

    /// Port the pin belongs to.
    #[inline]
    fn port(&self) -> Port {
        self._port()
    }

    /// Convert to the type-erased [`AnyPin`].
    #[inline]
    fn degrade(self) -> AnyPin {
        AnyPin {
            port: self._port(),
            pin: self._pin(),
        }
    }
}

pub(crate) trait SealedPin {
    fn _port(&self) -> Port;
    fn _pin(&self) -> u8;

    #[inline]
    fn block(&self) -> pac::gpio::Gpio {
        pac::GPIO
    }

    /// Read the input level.
    #[inline]
    fn is_high(&self) -> bool {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_pin().read().pa_pin(n),
            Port::B => self.block().pb_pin().read().pb_pin(n),
        }
    }

    /// Read the output latch.
    #[inline]
    fn is_set_high(&self) -> bool {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_out().read().pa_out(n),
            Port::B => self.block().pb_out().read().pb_out(n),
        }
    }

    /// Drive the pin high (write-1-to-set, so other pins are untouched).
    #[inline]
    fn set_high(&self) {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_set().write(|w| w.set_pa_set(n, true)),
            Port::B => self.block().pb_set().write(|w| w.set_pb_set(n, true)),
        }
    }

    /// Drive the pin low (write-1-to-clear).
    #[inline]
    fn set_low(&self) {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_clr().write(|w| w.set_pa_clr(n, true)),
            Port::B => self.block().pb_clr().write(|w| w.set_pb_clr(n, true)),
        }
    }

    /// Set the direction bit (1 = output).
    #[inline]
    fn set_direction(&self, output: bool) {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_dir().modify(|w| w.set_pa_dir(n, output)),
            Port::B => self.block().pb_dir().modify(|w| w.set_pb_dir(n, output)),
        }
    }

    /// Enable/disable the internal pull-up or pull-down.
    #[inline]
    fn set_pull(&self, pull: Pull) {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_pu().modify(|w| w.set_pa_pu(n, pull == Pull::Up)),
            Port::B => self.block().pb_pu().modify(|w| w.set_pb_pu(n, pull == Pull::Up)),
        }
        match self._port() {
            Port::A => self
                .block()
                .pa_pd_drv()
                .modify(|w| w.set_pa_pd_drv(n, matches!(pull, Pull::Down))),
            Port::B => self
                .block()
                .pb_pd_drv()
                .modify(|w| w.set_pb_pd_drv(n, matches!(pull, Pull::Down))),
        }
    }

    /// Set the drive strength (only meaningful for an output).
    #[inline]
    fn set_speed(&self, speed: Speed) {
        let n = self._pin() as usize;
        let high = speed == Speed::High;
        match self._port() {
            Port::A => self.block().pa_pd_drv().modify(|w| w.set_pa_pd_drv(n, high)),
            Port::B => self.block().pb_pd_drv().modify(|w| w.set_pb_pd_drv(n, high)),
        }
    }
}

impl Pin for AnyPin {}
impl SealedPin for AnyPin {
    #[inline]
    fn _port(&self) -> Port {
        self.port
    }
    #[inline]
    fn _pin(&self) -> u8 {
        self.pin
    }
}

foreach_pin!(
    ($pin_name:ident, $port_name:ident, $port_num:expr, $pin_num:expr) => {
        impl Pin for peripherals::$pin_name {}

        impl SealedPin for peripherals::$pin_name {
            #[inline]
            fn _port(&self) -> Port {
                // `$port_num` is 0 for A, 1 for B.
                if $port_num == 0 { Port::A } else { Port::B }
            }
            #[inline]
            fn _pin(&self) -> u8 {
                $pin_num
            }
        }

        impl From<peripherals::$pin_name> for AnyPin {
            fn from(x: peripherals::$pin_name) -> Self {
                x.degrade()
            }
        }
    };
);

/// GPIO flexible pin.
pub struct Flex<'d> {
    pub(crate) pin: Peri<'d, AnyPin>,
}

impl<'d> Flex<'d> {
    /// Wrap a pin without changing its configuration.
    #[inline]
    pub fn new(pin: Peri<'d, impl Pin>) -> Self {
        Self { pin: pin.into() }
    }

    /// Configure as an input with the given pull.
    #[inline]
    pub fn set_as_input(&mut self, pull: Pull) {
        critical_section::with(|_| {
            self.pin.set_direction(false);
            self.pin.set_pull(pull);
        });
    }

    /// Configure as a push-pull output.
    ///
    /// The output level is whatever was last latched, so set the level first if
    /// it matters.
    #[inline]
    pub fn set_as_output(&mut self, speed: Speed) {
        critical_section::with(|_| {
            self.pin.set_speed(speed);
            self.pin.set_direction(true);
        });
    }

    #[inline]
    pub fn is_high(&self) -> bool {
        self.pin.is_high()
    }

    #[inline]
    pub fn is_low(&self) -> bool {
        !self.is_high()
    }

    #[inline]
    pub fn get_level(&self) -> Level {
        self.is_high().into()
    }

    #[inline]
    pub fn is_set_high(&self) -> bool {
        self.pin.is_set_high()
    }

    #[inline]
    pub fn is_set_low(&self) -> bool {
        !self.is_set_high()
    }

    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.is_set_high().into()
    }

    #[inline]
    pub fn set_high(&mut self) {
        self.pin.set_high();
    }

    #[inline]
    pub fn set_low(&mut self) {
        self.pin.set_low();
    }

    #[inline]
    pub fn set_level(&mut self, level: Level) {
        match level {
            Level::Low => self.set_low(),
            Level::High => self.set_high(),
        }
    }

    /// Toggle the output.
    #[inline]
    pub fn toggle(&mut self) {
        if self.is_set_low() {
            self.set_high()
        } else {
            self.set_low()
        }
    }
}

/// GPIO input driver.
pub struct Input<'d> {
    pub(crate) pin: Flex<'d>,
}

impl<'d> Input<'d> {
    /// Create an input on `pin` with the given pull.
    #[inline]
    pub fn new(pin: Peri<'d, impl Pin>, pull: Pull) -> Self {
        let mut pin = Flex::new(pin);
        pin.set_as_input(pull);
        Self { pin }
    }

    /// Is the input high?
    #[inline]
    pub fn is_high(&self) -> bool {
        self.pin.is_high()
    }

    /// Is the input low?
    #[inline]
    pub fn is_low(&self) -> bool {
        self.pin.is_low()
    }

    /// Current input level.
    #[inline]
    pub fn get_level(&self) -> Level {
        self.pin.get_level()
    }
}

/// GPIO output driver.
pub struct Output<'d> {
    pub(crate) pin: Flex<'d>,
}

impl<'d> Output<'d> {
    /// Create an output on `pin`, driving `initial_output` at `speed`.
    #[inline]
    pub fn new(pin: Peri<'d, impl Pin>, initial_output: Level, speed: Speed) -> Self {
        let mut pin = Flex::new(pin);
        match initial_output {
            Level::High => pin.set_high(),
            Level::Low => pin.set_low(),
        }
        pin.set_as_output(speed);
        Self { pin }
    }

    #[inline]
    pub fn set_high(&mut self) {
        self.pin.set_high();
    }

    #[inline]
    pub fn set_low(&mut self) {
        self.pin.set_low();
    }

    #[inline]
    pub fn set_level(&mut self, level: Level) {
        self.pin.set_level(level)
    }

    #[inline]
    pub fn is_set_high(&self) -> bool {
        self.pin.is_set_high()
    }

    #[inline]
    pub fn is_set_low(&self) -> bool {
        self.pin.is_set_low()
    }

    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.pin.get_output_level()
    }

    /// Toggle the output.
    #[inline]
    pub fn toggle(&mut self) {
        self.pin.toggle();
    }
}

/// Enable the GPIO block.
///
/// There is no peripheral clock gate to turn on — the CH5xx GPIO is always
/// clocked. All this does is keep the same call shape as ch32-hal so `init` can
/// treat it uniformly.
pub(crate) unsafe fn init(_cs: CriticalSection) {
    // Nothing to program: the CH5xx GPIO block is always clocked and every pin
    // comes out of reset as a floating input. Kept so `init` has the same shape
    // as ch32-hal's.
}

// --- embedded-hal impls ---------------------------------------------------

impl<'d> embedded_hal::digital::ErrorType for Input<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::InputPin for Input<'d> {
    #[inline]
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    #[inline]
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for Output<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::OutputPin for Output<'d> {
    #[inline]
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_high())
    }

    #[inline]
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_low())
    }
}

impl<'d> embedded_hal::digital::StatefulOutputPin for Output<'d> {
    #[inline]
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_high())
    }

    #[inline]
    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for Flex<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::InputPin for Flex<'d> {
    #[inline]
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    #[inline]
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::OutputPin for Flex<'d> {
    #[inline]
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_high())
    }

    #[inline]
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_low())
    }
}

impl<'d> embedded_hal::digital::StatefulOutputPin for Flex<'d> {
    #[inline]
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_high())
    }

    #[inline]
    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_low())
    }
}
