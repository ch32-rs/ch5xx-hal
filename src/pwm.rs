//! PWM4 to PWM11.
//!
//! One block clocks every channel from `Fsys`. `CLOCK_DIV` divides that clock,
//! and the cycle length is shared too. The output frequency is
//! `Fsys / divisor / cycle`. A divisor of 0 is rejected.
//!
//! Fixed cycles use 8-bit duty registers. On CH582/CH583, the available
//! periods are 256, 255, 128, 127, 64, 63, 32 and 31 clocks. On CH581,
//! CH584, CH585 and CH592, the last mode instead provides a custom 16-bit
//! cycle for PWM4 to PWM9; PWM10 and PWM11 are unavailable in that mode.
//!
//! Duty 0 leaves the pin at its idle level; the largest duty is the cycle
//! length, except for the 256-clock cycle, whose byte register stops at 255.
//! Staggered output can alternate the channels of each pair between cycles.
//!
//! `PIN_PWMX` remaps PWM4, PWM5, PWM7, PWM8 and PWM9 together. Mixing a
//! remapped pin with a default one flips the shared bit, so every channel has
//! to agree. PWM11 on CH582/CH583 shares PB23 with external reset; that function
//! must be disabled before using the pin for PWM. The block raises
//! `PWMX_SPI1`, the same line as SPI1; this driver does not claim it.

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
    /// The custom cycle is zero clocks.
    Cycle,
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
    /// 32 clocks (CH582/CH583).
    #[cfg(pwmx_ch573)]
    Clocks32,
    /// 31 clocks (CH582/CH583).
    #[cfg(pwmx_ch573)]
    Clocks31,
    /// A custom period of `n` clocks, 1 to 65535, using the 16-bit counter
    /// mode. `CONFIG.CYC_MOD` selects it and `REG_CYCLE` holds `n - 1`.
    ///
    /// The duty registers widen to 16 bits, which only PWM4 to PWM9 have;
    /// [`Pwm::channel`] rejects PWM10 and PWM11 in this mode.
    #[cfg(any(pwmx_ch585, pwmx_ch592))]
    Clocks16(u16),
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
            #[cfg(pwmx_ch573)]
            Cycle::Clocks32 => 32,
            #[cfg(pwmx_ch573)]
            Cycle::Clocks31 => 31,
            #[cfg(any(pwmx_ch585, pwmx_ch592))]
            Cycle::Clocks16(clocks) => clocks,
        }
    }

    /// Largest value the duty register can hold for this cycle.
    const fn max_duty(self) -> u16 {
        match self {
            Cycle::Clocks256 => 255,
            #[cfg(any(pwmx_ch585, pwmx_ch592))]
            Cycle::Clocks16(clocks) => clocks,
            other => other.clocks(),
        }
    }

    /// Whether the duty registers are 16 bits wide.
    const fn wide(self) -> bool {
        #[cfg(any(pwmx_ch585, pwmx_ch592))]
        {
            matches!(self, Cycle::Clocks16(_))
        }
        #[cfg(pwmx_ch573)]
        {
            false
        }
    }

    /// `CONFIG.CYC_MOD` and `CONFIG.CYCLE_SEL` for this register layout.
    fn fields(self) -> (u8, bool) {
        match self {
            Cycle::Clocks256 => (0, false),
            Cycle::Clocks255 => (0, true),
            Cycle::Clocks128 => (1, false),
            Cycle::Clocks127 => (1, true),
            Cycle::Clocks64 => (2, false),
            Cycle::Clocks63 => (2, true),
            #[cfg(pwmx_ch573)]
            Cycle::Clocks32 => (3, false),
            #[cfg(pwmx_ch573)]
            Cycle::Clocks31 => (3, true),
            #[cfg(any(pwmx_ch585, pwmx_ch592))]
            Cycle::Clocks16(_) => (3, false),
        }
    }
}

/// A channel pair that can take turns driving its outputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StaggerPair {
    /// PWM4 and PWM5.
    Pwm4_5,
    /// PWM6 and PWM7.
    Pwm6_7,
    /// PWM8 and PWM9.
    Pwm8_9,
    /// PWM10 and PWM11.
    Pwm10_11,
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
        if config.cycle.clocks() == 0 {
            return Err(ConfigError::Cycle);
        }
        let regs = T::regs();
        regs.clock_div().write(|w| w.set_clock_div(config.divisor));
        #[cfg(any(pwmx_ch585, pwmx_ch592))]
        if let Cycle::Clocks16(clocks) = config.cycle {
            // The hardware counts one more clock than the register holds.
            regs.reg_cycle().write(|w| w.set_cyc_value(clocks - 1));
        }
        let (width, short) = config.cycle.fields();
        // Leave the stagger-enable bits alone. They are not part of the cycle.
        critical_section::with(|_| {
            regs.config().modify(|w| {
                w.set_cyc_mod(width);
                w.set_cycle_sel(short);
            });
        });
        Ok(Self {
            cycle: config.cycle,
            _phantom: PhantomData,
        })
    }

    /// Enable or disable alternate output for a channel pair.
    ///
    /// While a pair is enabled the two channels take turns: the even channel
    /// (PWM4/6/8/10) drives its pin on one PWM cycle, the odd channel
    /// (PWM5/7/9/11) on the next. Both keep their own duty, polarity and
    /// output enable. Toggling the pair changes no duty.
    ///
    /// The setting is block-wide and takes `&self`, so it can be changed while
    /// channels are live.
    pub fn set_stagger(&self, pair: StaggerPair, enabled: bool) {
        critical_section::with(|_| {
            T::regs().config().modify(|w| match pair {
                StaggerPair::Pwm4_5 => w.set_pwm4_5_stag_en(enabled),
                StaggerPair::Pwm6_7 => w.set_pwm6_7_stag_en(enabled),
                StaggerPair::Pwm8_9 => w.set_pwm8_9_stag_en(enabled),
                StaggerPair::Pwm10_11 => w.set_pwm10_11_stag_en(enabled),
            });
        });
    }

    /// Which half of the stagger pairs owns the output window
    /// (`CONFIG.STAG_ST`): `true` while PWM5/7/9/11 may drive, `false` while
    /// PWM4/6/8/10 may.
    pub fn stagger_phase(&self) -> bool {
        T::regs().config().read().stag_st()
    }

    /// Drive `pin` from its channel.
    ///
    /// The returned output borrows the block, so the clock cannot be rebuilt
    /// while the pin is live. Dropping it releases the pin and clears the
    /// channel's output enable.
    ///
    /// # Panics
    ///
    /// On blocks with 16-bit mode, panics when the block is in that mode and the pin is
    /// PWM10 or PWM11, which have no 16-bit duty register.
    pub fn channel<'a, P: PwmPin<T>>(&'a self, pin: Peri<'a, P>, polarity: Polarity) -> Channel<'a, T> {
        assert!(
            !(self.cycle.wide() && P::INDEX > 5),
            "16-bit cycle mode has no duty register for PWM10/PWM11"
        );
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
        set_duty::<T>(P::INDEX, 0, self.cycle.wide());
        set_output::<T>(P::INDEX, true);
        Channel {
            index: P::INDEX,
            max_duty: self.cycle.max_duty(),
            wide: self.cycle.wide(),
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
    wide: bool,
    pin: Flex<'a>,
    _instance: PhantomData<&'a T>,
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
        set_duty::<T>(self.index, duty.min(self.max_duty), self.wide);
        Ok(())
    }
}

fn set_duty<T: Instance>(index: u8, duty: u16, wide: bool) {
    // Some layouts pack multiple channels into one register, so their updates
    // must not interrupt each other's read-modify-write sequences.
    critical_section::with(|_| {
        #[cfg(any(pwmx_ch585, pwmx_ch592))]
        if wide {
            set_duty_wide::<T>(index, duty);
            return;
        }
        #[cfg(pwmx_ch573)]
        let _ = wide;
        set_duty_narrow::<T>(index, duty as u8);
    });
}

fn set_duty_narrow<T: Instance>(index: u8, duty: u8) {
    let regs = T::regs();
    #[cfg(pwmx_ch585)]
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
    // CH582/CH583 and CH592 expose standalone 8-bit registers.
    #[cfg(any(pwmx_ch573, pwmx_ch592))]
    match index {
        0 => regs.pwm4_data().write(|w| w.set_pwm4_data(duty)),
        1 => regs.pwm5_data().write(|w| w.set_pwm5_data(duty)),
        2 => regs.pwm6_data().write(|w| w.set_pwm6_data(duty)),
        3 => regs.pwm7_data().write(|w| w.set_pwm7_data(duty)),
        4 => regs.pwm8_data().write(|w| w.set_pwm8_data(duty)),
        5 => regs.pwm9_data().write(|w| w.set_pwm9_data(duty)),
        6 => regs.pwm10_data().write(|w| w.set_pwm10_data(duty)),
        _ => regs.pwm11_data().write(|w| w.set_pwm11_data(duty)),
    }
}

/// Write a 16-bit duty. PWM4 to PWM7 read their two bytes as one word at
/// 0x04, 0x06, 0x08 and 0x0A; PWM8 and PWM9 have 16-bit fields in `REG_DATA8`.
/// The caller has already rejected PWM10 and PWM11.
#[cfg(any(pwmx_ch585, pwmx_ch592))]
fn set_duty_wide<T: Instance>(index: u8, duty: u16) {
    let regs = T::regs();
    match index {
        0..=3 => set_duty_pair::<T>(index, duty as u8, (duty >> 8) as u8),
        4 => regs.reg_data8().modify(|w| w.set_pwm8_reg_data(duty)),
        5 => regs.reg_data8().modify(|w| w.set_pwm9_reg_data(duty)),
        _ => {}
    }
}

/// The CH585 layout joins the bytes in 32-bit words, so one `modify` writes
/// both halves of the duty without a transient value.
#[cfg(pwmx_ch585)]
fn set_duty_pair<T: Instance>(index: u8, low: u8, high: u8) {
    let regs = T::regs();
    match index {
        0 => regs.pwm4_7_data().modify(|w| {
            w.set_pwm4_data__pwm4_data(low);
            w.set_pwm5_data__pwm4_data(high);
        }),
        1 => regs.pwm4_7_data().modify(|w| {
            w.set_pwm6_data__pwm5_data(low);
            w.set_pwm7_data__pwm5_data(high);
        }),
        2 => regs.pwm8_11_data().modify(|w| {
            w.set_pwm8_data__pwm6_data(low);
            w.set_pwm9_data__pwm6_data(high);
        }),
        _ => regs.pwm8_11_data().modify(|w| {
            w.set_pwm10_data__pwm7_data(low);
            w.set_pwm11_data__pwm7_data(high);
        }),
    }
}

/// CH592 has no 16-bit alias for the pair: low byte first, then high byte.
/// The cycle in progress may see one mixed value.
#[cfg(pwmx_ch592)]
fn set_duty_pair<T: Instance>(index: u8, low: u8, high: u8) {
    let regs = T::regs();
    match index {
        0 => {
            regs.pwm4_data().write(|w| w.set_pwm4_data(low));
            regs.pwm5_data().write(|w| w.set_pwm5_data(high));
        }
        1 => {
            regs.pwm6_data().write(|w| w.set_pwm6_data(low));
            regs.pwm7_data().write(|w| w.set_pwm7_data(high));
        }
        2 => {
            regs.pwm8_data().write(|w| w.set_pwm8_data(low));
            regs.pwm9_data().write(|w| w.set_pwm9_data(high));
        }
        _ => {
            regs.pwm10_data().write(|w| w.set_pwm10_data(low));
            regs.pwm11_data().write(|w| w.set_pwm11_data(high));
        }
    }
}

fn set_polarity<T: Instance>(index: u8, polarity: Polarity) {
    let active_low = polarity == Polarity::ActiveLow;
    let regs = T::regs();
    critical_section::with(|_| {
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
    });
}

fn set_output<T: Instance>(index: u8, enabled: bool) {
    let regs = T::regs();
    critical_section::with(|_| {
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

#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PA12, 0, Some(false));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PA6, 0, Some(true));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PA13, 1, Some(false));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PA7, 1, Some(true));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB0, 2, None);
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB4, 3, Some(false));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB1, 3, Some(true));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB6, 4, Some(false));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB2, 4, Some(true));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB7, 5, Some(false));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB3, 5, Some(true));
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB14, 6, None);
#[cfg(any(ch581, ch582, ch583, ch584, ch585, ch592))]
impl_pin!(PB23, 7, None);
