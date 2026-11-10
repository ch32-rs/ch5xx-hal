//! Capacitive touch keys.
//!
//! The touch-key block is the ADC's charge-transfer front end. One acquisition
//! charges the selected pad for [`Config::charge`] ADC clocks, moves that charge
//! onto the sampling capacitor for [`Config::discharge`] clocks, and leaves the
//! result in `ADC.DATA`. The pad is selected through the ADC channel mux, so a
//! [`Pad`] is just an [`AdcPin`] held as a floating input.
//!
//! The result is a count, not a voltage, and it follows the pad capacitance: a
//! finger adds capacitance and moves the count. Compare it with an untouched
//! baseline and a per-pad threshold instead of converting it.
//!
//! The block shares the converter with [`crate::adc`], so a [`TouchKey`] owns
//! both the touch-key and the ADC peripheral and an [`crate::adc::Adc`] cannot
//! exist at the same time. The ADC interrupt line is shared as well; this
//! driver polls and does not enable it.
//!
//! CH595 replaced `COUNT`/`CONVERT` with the `CFG2` transfer registers and
//! CH572 has no touch-key block, so the module only compiles for the dies that
//! keep the register set. Only CH592 has been run on a board.

use crate::adc::{self, AdcPin};
use crate::gpio::{Flex, Pull};
use crate::{pac, peripherals, Peri};

/// `ADC.DATA` is 12 bits wide.
const DATA_MASK: u16 = 0x0fff;

/// Charge-transfer lengths and converter setup.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Config {
    /// Charge clocks, `COUNT.CHARG_CNT`. 1 to 31.
    ///
    /// Longer charging raises the untouched count and the swing a finger
    /// produces, at the cost of headroom: stop before `ADC.DATA` reaches 4095.
    pub charge: u8,
    /// Discharge clocks, `COUNT.DISCH_CNT`. 0 to 7.
    pub discharge: u8,
    /// Select the 70 µA charge current (`CFG.CURRENT`) instead of 35 µA.
    ///
    /// The CH592 vendor touch library selects the higher current; with the
    /// lower one a small pad can saturate `ADC.DATA` after one or two charge
    /// clocks. The CH582/CH583 SDK preserves the existing current setting.
    pub high_current: bool,
    /// Enable the drive shield (`CFG.DRV_EN`), when the board wires one.
    pub shield: bool,
    /// Converter gain and clock. The vendor touch-key setup uses the defaults.
    pub adc: adc::Config,
}

impl Default for Config {
    fn default() -> Self {
        // The vendor touch library's working point: two charge clocks, three
        // discharge clocks and the 70 µA current.
        let adc = cfg_select! {
            // CH582/CH583's vendor touch setup uses -6 dB and the first clock
            // setting, unlike the newer ADC blocks.
            adc_ch583 => adc::Config {
                gain: adc::Gain::Div2,
                sample_clock: adc::SampleClock::Divide1,
            },
            // CH573's vendor touch setup keeps unity gain and the slowest clock.
            adc_ch573 => adc::Config {
                gain: adc::Gain::Unity,
                sample_clock: adc::SampleClock::Divide1,
            },
            _ => adc::Config::default(),
        };
        Self {
            charge: 2,
            discharge: 3,
            high_current: true,
            shield: false,
            adc,
        }
    }
}

/// Why a configuration was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The charge length is outside 1..=31.
    Charge,
    /// The discharge length is outside 0..=7.
    Discharge,
}

/// Why an acquisition did not complete.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Error {
    /// The block did not clear `CONVERT.START` within ten milliseconds.
    Timeout,
}

/// The touch-key front end and its converter.
pub struct TouchKey<'d> {
    _tkey: Peri<'d, peripherals::TOUCHKEY>,
    _adc: Peri<'d, peripherals::ADC>,
    config: Config,
}

impl<'d> TouchKey<'d> {
    /// Power the converter and the touch-key front end.
    ///
    /// The ADC channel is selected by every [`TouchKey::read`], so the
    /// constructor only brings the converter up with the requested gain and
    /// clock.
    pub fn new(
        tkey: Peri<'d, peripherals::TOUCHKEY>,
        adc: Peri<'d, peripherals::ADC>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        if config.charge == 0 || config.charge > 31 {
            return Err(ConfigError::Charge);
        }
        if config.discharge > 7 {
            return Err(ConfigError::Discharge);
        }
        adc::enable::<peripherals::ADC>(0, config.adc, false);
        pac::TOUCHKEY.cfg().write(|w| {
            w.set_pwr_on(true);
            w.set_current(config.high_current);
            // CH573 has no drive shield.
            #[cfg(touchkey_drv_en)]
            w.set_drv_en(config.shield);
        });
        Ok(Self {
            _tkey: tkey,
            _adc: adc,
            config,
        })
    }

    /// Run one acquisition on `pad`.
    ///
    /// The first acquisition after power-up or a channel change can be stale;
    /// average several readings instead of trusting one.
    pub fn read(&mut self, pad: &mut Pad<'_>) -> Result<u16, Error> {
        let regs = pac::ADC;
        regs.channel().write(|w| w.set_ch_inx(pad.channel));
        let tkey = pac::TOUCHKEY;
        tkey.count().write(|w| {
            w.set_charg_cnt(self.config.charge);
            w.set_disch_cnt(self.config.discharge);
        });
        tkey.convert().write(|w| w.set_start(true));
        let deadline = adc::deadline_in(10);
        while tkey.convert().read().start() {
            if adc::timed_out(deadline) {
                return Err(Error::Timeout);
            }
        }
        Ok(regs.data().read().data() & DATA_MASK)
    }
}

impl Drop for TouchKey<'_> {
    fn drop(&mut self) {
        pac::TOUCHKEY.cfg().modify(|w| w.set_pwr_on(false));
        pac::ADC.cfg().modify(|w| w.set_power_on(false));
    }
}

/// A pad wired to an ADC channel, held as a floating input.
pub struct Pad<'d> {
    _pin: Flex<'d>,
    channel: u8,
}

impl<'d> Pad<'d> {
    /// Hold `pin` as a floating input, so its digital driver does not load the
    /// pad, and remember its ADC channel.
    pub fn new<P: AdcPin<peripherals::ADC>>(pin: Peri<'d, P>) -> Self {
        let mut pin = Flex::new(pin.into());
        pin.set_as_input(Pull::None);
        Self {
            _pin: pin,
            channel: P::channel().0,
        }
    }

    /// The ADC channel the pad is wired to.
    pub const fn channel(&self) -> u8 {
        self.channel
    }
}
