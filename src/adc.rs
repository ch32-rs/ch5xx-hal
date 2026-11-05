//! ADC single conversions.
//!
//! CH585 has one converter, clocked from its own divider rather than `Fsys`.
//! This driver does one conversion at a time and polls `CONVERT.EOC_X`. The DMA,
//! scan and touch-key modes share the same block and are left untouched.
//!
//! A channel has to be selected before the converter is powered, which is the
//! order the vendor setup uses. An external channel also disconnects the pad's
//! digital input through `PIN_IN_DIS`, or the pin driver fights the signal being
//! measured. The converter is powered down on drop and the pad is reconnected.
//!
//! The raw result is 12 bits. The vendor notes an offset near the ends of the
//! range; [`Adc::read`] returns the sample unchanged, and [`Adc::calibrate`]
//! measures that offset so a caller can add it.
//!
//! The register block differs between dies. The module compiles for the CH585
//! layout (CH581, CH584, CH585) and is absent on the rest. Only CH585 has been
//! run on a board.

use core::marker::PhantomData;

use embassy_hal_internal::PeripheralType;

use crate::gpio::{Flex, Pin, Pull};
use crate::{interrupt, pac, peripherals, Peri};

/// Errors a conversion can return.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Error {
    /// The converter did not finish within one millisecond.
    Timeout,
}

/// Input gain. The names are the ratios the vendor documents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gain {
    /// -12 dB, one quarter.
    Div4,
    /// -6 dB, one half.
    Div2,
    /// 0 dB, unity. The vendor's default for an external channel.
    Unity,
    /// 6 dB, two.
    X2,
    /// 12 dB, four.
    X4,
    /// 18 dB, eight.
    X8,
    /// 24 dB, sixteen.
    X16,
}

impl Gain {
    /// The two `CFG.PGA_GAIN10` bits and the `CONVERT.PGA_GAIN2` direction bit.
    fn fields(self) -> (u8, bool) {
        // The low two bits select -12, -6, 0 and 6 dB. The direction bit shifts
        // that selection up, so 6 dB is the low bits alone and the higher gains
        // set the direction bit. This is the vendor's `ADC_PGA_*` encoding.
        match self {
            Gain::Div4 => (0, false),
            Gain::Div2 => (1, false),
            Gain::Unity => (2, false),
            Gain::X2 => (3, false),
            Gain::X4 => (1, true),
            Gain::X8 => (2, true),
            Gain::X16 => (3, true),
        }
    }
}

/// Converter clock divider, from the vendor's sampling-frequency enum.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum SampleClock {
    /// The fastest setting, which the vendor restricts to a 32 MHz clock source.
    Divide1 = 0,
    /// 8 MHz from a 32 MHz source, 4 MHz otherwise. The vendor's default.
    Divide2 = 1,
    /// 5.33 MHz from a 32 MHz source, 2.67 MHz otherwise.
    Divide3 = 2,
    /// 4 MHz from a 32 MHz source, 2 MHz otherwise.
    Divide4 = 3,
}

/// Converter configuration.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Config {
    /// Input gain.
    pub gain: Gain,
    /// Converter clock.
    pub sample_clock: SampleClock,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gain: Gain::Unity,
            sample_clock: SampleClock::Divide2,
        }
    }
}

/// A single-conversion ADC.
pub struct Adc<'d, T: Instance> {
    channel: ChannelKind<'d>,
    _phantom: PhantomData<&'d mut T>,
}

/// What the selected channel is, so drop can put it back.
enum ChannelKind<'d> {
    /// An external pin whose digital input was disconnected.
    Pin(Flex<'d>),
    /// The internal temperature sensor, which was powered on.
    Temperature,
    /// The divided battery voltage. Nothing was powered on for it.
    Battery,
}

impl<'d, T: Instance> Adc<'d, T> {
    /// Measure one external pin.
    ///
    /// The pad is floated and its digital input is disconnected before the
    /// converter powers up.
    pub fn new_pin<P: AdcPin<T>>(peri: Peri<'d, T>, pin: Peri<'d, P>, config: Config) -> Self {
        let (channel, _pin) = P::channel();
        let mut pin = Flex::new(pin.into());
        pin.set_as_input(Pull::None);
        disconnect(channel);
        enable::<T>(channel, config, false);
        let _ = peri;
        Self {
            channel: ChannelKind::Pin(pin),
            _phantom: PhantomData,
        }
    }

    /// Measure the internal temperature sensor.
    ///
    /// The vendor setup forces differential mode and a gain of two for this
    /// channel, so `config` is not used. [`Adc::temperature`] turns the raw
    /// sample into a temperature with the value stored in the info flash.
    pub fn new_temperature(peri: Peri<'d, T>) -> Self {
        T::regs().tem_sensor().write(|w| w.set_tem_sen_pwr_on(true));
        enable::<T>(TEMPERATURE_CHANNEL, temperature_config(), true);
        let _ = peri;
        Self {
            channel: ChannelKind::Temperature,
            _phantom: PhantomData,
        }
    }

    /// Measure the divided battery voltage.
    ///
    /// The vendor setup uses a gain of one quarter and the input buffer, since
    /// the battery is divided down to the converter's range.
    pub fn new_battery(peri: Peri<'d, T>, config: Config) -> Self {
        enable::<T>(BATTERY_CHANNEL, config, false);
        let _ = peri;
        Self {
            channel: ChannelKind::Battery,
            _phantom: PhantomData,
        }
    }

    /// One conversion, as a raw 12-bit sample.
    ///
    /// The first conversion after power-up is stale. The vendor discards it, and
    /// so does this: a call takes two conversions and returns the second.
    pub fn read(&mut self) -> Result<u16, Error> {
        let _ = self.convert()?;
        self.convert()
    }

    /// The converter's own offset, from the vendor's rough calibration.
    ///
    /// Sixteen conversions of the shorted input are averaged and subtracted from
    /// the mid-scale value. Add the result to a [`Adc::read`] sample. The channel
    /// and gain are restored afterwards.
    pub fn calibrate(&mut self) -> Result<i16, Error> {
        let regs = T::regs();
        let channel = regs.channel().read();
        let config = regs.cfg().read();
        regs.cfg().modify(|w| {
            w.set_ofs_test(true);
            w.set_diff_en(false);
        });
        let _ = self.convert()?;
        let mut sum = 0u32;
        for _ in 0..16 {
            // The offset test reports the complement of the data bits.
            sum += u32::from(!self.convert()? & DATA_MASK);
        }
        regs.channel().write_value(channel);
        regs.cfg().write_value(config);
        Ok(2048 - ((sum + 8) >> 4) as i16)
    }

    /// Degrees Celsius from a temperature-channel sample.
    ///
    /// The info flash holds the sample the sensor produced at its reference
    /// temperature, and that temperature in the upper half when it is not zero.
    /// The vendor slope is 283 counts per 100 degrees. A converter that is not
    /// on the temperature channel still computes the number, but it does not
    /// mean anything.
    pub fn temperature(&self, sample: u16) -> i32 {
        let calib = unsafe { (TEMPERATURE_CALIB as *const u32).read_volatile() };
        let reference = i32::from((calib >> 16) as u16);
        let reference = if reference == 0 { 25 } else { reference };
        let counts = i32::from(calib as u16);
        reference + (i32::from(sample) - counts) * 100 / 283
    }

    fn convert(&mut self) -> Result<u16, Error> {
        let regs = T::regs();
        regs.convert().modify(|w| w.set_start(true));
        let deadline = deadline();
        loop {
            let state = regs.convert().read();
            if !state.start() && !state.eoc_x() {
                return Ok(u16::from(regs.data().read().data()));
            }
            if timed_out(deadline) {
                return Err(Error::Timeout);
            }
        }
    }
}

impl<'d, T: Instance> Drop for Adc<'d, T> {
    fn drop(&mut self) {
        let regs = T::regs();
        regs.cfg().modify(|w| w.set_power_on(false));
        match &mut self.channel {
            ChannelKind::Pin(pin) => reconnect(pin.pin.port() as u8, pin.pin.pin()),
            ChannelKind::Temperature => regs.tem_sensor().write(|w| w.set_tem_sen_pwr_on(false)),
            ChannelKind::Battery => {}
        }
    }
}

/// `DATA` is 12 bits wide.
const DATA_MASK: u16 = 0x0fff;

/// `CH_INTE_VBAT` in the vendor header.
const BATTERY_CHANNEL: u8 = 14;

/// `CH_INTE_VTEMP` in the vendor header.
const TEMPERATURE_CHANNEL: u8 = 15;

/// Factory temperature calibration in the info flash, `ROM_CFG_TMP_25C`.
const TEMPERATURE_CALIB: u32 = 0x0007_f014;

/// Power the converter up on `channel`.
///
/// Touch-key power is cleared first, because the vendor setup treats the two as
/// mutually exclusive. `differential` selects the input mode the temperature
/// sensor needs.
fn enable<T: Instance>(channel: u8, config: Config, differential: bool) {
    let regs = T::regs();
    // The touch-key block is the other user of this converter, and the vendor
    // setup powers it down before an ADC conversion.
    pac::TOUCHKEY.cfg().modify(|w| w.set_pwr_on(false));
    regs.channel().write(|w| w.set_ch_inx(channel));
    let (gain, gain2) = config.gain.fields();
    regs.cfg().write(|w| {
        w.set_buf_en(!differential);
        w.set_diff_en(differential);
        w.set_pga_gain10(gain);
        w.set_clk_div(config.sample_clock as u8);
    });
    regs.convert().modify(|w| w.set_pga_gain2(gain2));
    regs.cfg().modify(|w| w.set_power_on(true));
}

/// The configuration the vendor forces for the temperature sensor.
fn temperature_config() -> Config {
    Config {
        gain: Gain::X2,
        sample_clock: SampleClock::Divide1,
    }
}

/// Disconnect the digital input of an ADC pin, so the pad driver is out of the way.
fn disconnect(channel: u8) {
    let (port, pin) = channel_pin(channel);
    pac::SYS.pin_in_dis().modify(|w| {
        let mut mask = if port == 0 { w.pax_in_dis() } else { w.pblx_in_dis() };
        mask |= 1 << pin;
        if port == 0 {
            w.set_pax_in_dis(mask);
        } else {
            w.set_pblx_in_dis(mask);
        }
    });
}

/// Reconnect the digital input of the pin a channel borrowed.
fn reconnect(port: u8, pin: u8) {
    pac::SYS.pin_in_dis().modify(|w| {
        let mut mask = if port == 0 { w.pax_in_dis() } else { w.pblx_in_dis() };
        mask &= !(1 << pin);
        if port == 0 {
            w.set_pax_in_dis(mask);
        } else {
            w.set_pblx_in_dis(mask);
        }
    });
}

/// The port and pin behind a channel index. Port 0 is A.
fn channel_pin(channel: u8) -> (u8, u8) {
    // Datasheet table 7-8: A0 is PA4, then PA5, PA12, PA13, PA14, PA15, PA3,
    // PA2, PA1, PA0, PA6, PA7.
    let pin = match channel {
        0 => 4,
        1 => 5,
        2 => 12,
        3 => 13,
        4 => 14,
        5 => 15,
        6 => 3,
        7 => 2,
        8 => 1,
        9 => 0,
        10 => 6,
        _ => 7,
    };
    (0, pin)
}

/// One millisecond from now, in system-clock cycles.
fn deadline() -> u64 {
    cycles() + u64::from(crate::rcc::clocks().sysclk.0) / 1000
}

fn timed_out(deadline: u64) -> bool {
    cycles().wrapping_sub(deadline) < (1 << 63)
}

fn cycles() -> u64 {
    let mut high = 0u32;
    let mut low = 0u32;
    unsafe {
        core::arch::asm!(
            "1:",
            "csrr {high}, 0xC80",
            "csrr {low}, 0xC00",
            "csrr {check}, 0xC80",
            "bne {high}, {check}, 1b",
            high = out(reg) high,
            low = out(reg) low,
            check = out(reg) _,
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

// --- instance and pins -----------------------------------------------------

mod sealed {
    use super::pac;

    pub trait Instance {
        fn regs() -> pac::adc::Adc;
    }
}

/// An ADC with the CH585 register layout.
#[allow(private_bounds)]
pub trait Instance: sealed::Instance + PeripheralType {
    /// Interrupt raised when a conversion ends.
    ///
    /// Named for a later async driver. The touch-key block shares this line.
    type Interrupt: interrupt::typelevel::Interrupt;
}

impl sealed::Instance for peripherals::ADC {
    fn regs() -> pac::adc::Adc {
        pac::ADC
    }
}

impl Instance for peripherals::ADC {
    type Interrupt = interrupt::typelevel::ADC;
}

mod pin_sealed {
    pub trait Channel {
        /// The channel index and the `(port, pin)` whose digital input to cut.
        fn index() -> (u8, u8);
    }
}

/// A pin that can be an ADC input on `T`.
pub trait AdcPin<T: Instance>: Pin + pin_sealed::Channel {
    /// The channel index and the pad bit this pin measures.
    fn channel() -> (u8, u8) {
        <Self as pin_sealed::Channel>::index()
    }
}

macro_rules! impl_channel {
    ($pin:ident, $channel:literal, $bit:literal) => {
        impl pin_sealed::Channel for peripherals::$pin {
            fn index() -> (u8, u8) {
                ($channel, $bit)
            }
        }
        impl AdcPin<peripherals::ADC> for peripherals::$pin {}
    };
}

// The channel number is the pin's index in the family pin list.
impl_channel!(PA4, 0, 4);
impl_channel!(PA5, 1, 5);
impl_channel!(PA12, 2, 12);
impl_channel!(PA13, 3, 13);
impl_channel!(PA14, 4, 14);
impl_channel!(PA15, 5, 15);
impl_channel!(PA3, 6, 3);
impl_channel!(PA2, 7, 2);
impl_channel!(PA1, 8, 1);
impl_channel!(PA0, 9, 0);
impl_channel!(PA6, 10, 6);
impl_channel!(PA7, 11, 7);
