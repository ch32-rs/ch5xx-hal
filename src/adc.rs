//! ADC conversions and scans.
//!
//! CH585 has one converter, clocked from its own divider rather than `Fsys`.
//! [`Adc`] does one conversion at a time and polls `CONVERT.EOC_X`. [`Scan`]
//! samples an ordered sequence of external channels and stores the samples with
//! the block's local DMA engine; one pass can cover up to fourteen channels.
//! Touch-key mode shares the same converter and is left untouched.
//!
//! Scan completion is polled from [`ScanTransfer::blocking_wait`]. The ADC
//! interrupt line is asserted by end-of-conversion as well as by the scan and
//! DMA end flags, and the documented `IE_*` bits do not mask that source, so an
//! interrupt-driven wait would spend its time in a handler that cannot clear the
//! line without disturbing the sequence. A TouchKey driver taking over the line
//! must account for this.
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

use crate::dma::Descriptor;
use crate::gpio::{Flex, Pin, Pull};
use crate::{interrupt, pac, peripherals, Peri};

/// Errors a conversion can return.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Error {
    /// The converter did not finish within one millisecond.
    Timeout,
    /// A scan buffer is empty, is not a whole number of passes, or is not in
    /// the DMA SRAM window.
    Buffer,
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
        convert_once::<T>()
    }
}

/// One conversion, polling `CONVERT` for completion.
fn convert_once<T: Instance>() -> Result<u16, Error> {
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

// --- scan and local DMA ----------------------------------------------------

/// Sequence entries in `SCAN_CFG1`, after the one held by `CHANNEL`.
const SCAN_CFG1_ENTRIES: usize = 8;

/// Sequence entries in `SCAN_CFG2`, after `SCAN_CFG1`.
const SCAN_CFG2_ENTRIES: usize = 5;

/// The largest scan sequence the sequencer accepts: `CHANNEL` plus the two
/// sequence registers.
pub const MAX_CHANNELS: usize = 1 + SCAN_CFG1_ENTRIES + SCAN_CFG2_ENTRIES;

mod scan_sealed {
    use super::{Instance, MAX_CHANNELS};

    pub(crate) trait Pins<T: Instance> {
        const COUNT: usize;
        fn channels(sequence: &mut [u8; MAX_CHANNELS]);
    }
}

/// A tuple of ADC pins, scanned in tuple order.
///
/// Tuples of one to [`MAX_CHANNELS`] pins are supported. The order of the tuple
/// is the order of the sequence: the first element comes from `CHANNEL.CH_INX`,
/// the next eight from `SCAN_CFG1` and the rest from `SCAN_CFG2`. The pins are
/// configured as floating analog inputs when the [`Scan`] is created, and their
/// digital inputs are reconnected when it is dropped.
#[allow(private_bounds)]
pub trait ScanPins<T: Instance>: scan_sealed::Pins<T> {}

macro_rules! impl_scan_pins {
    ($count:literal, $($pin:ident),+) => {
        impl<'d, T: Instance, $($pin: AdcPin<T>),+> scan_sealed::Pins<T> for ($(Peri<'d, $pin>,)+) {
            const COUNT: usize = $count;

            fn channels(sequence: &mut [u8; MAX_CHANNELS]) {
                let mut index = 0;
                $(
                    sequence[index] = $pin::channel().0;
                    index += 1;
                )+
                let _ = index;
            }
        }

        impl<'d, T: Instance, $($pin: AdcPin<T>),+> ScanPins<T> for ($(Peri<'d, $pin>,)+) {}
    };
}

impl_scan_pins!(1, P1);
impl_scan_pins!(2, P1, P2);
impl_scan_pins!(3, P1, P2, P3);
impl_scan_pins!(4, P1, P2, P3, P4);
impl_scan_pins!(5, P1, P2, P3, P4, P5);
impl_scan_pins!(6, P1, P2, P3, P4, P5, P6);
impl_scan_pins!(7, P1, P2, P3, P4, P5, P6, P7);
impl_scan_pins!(8, P1, P2, P3, P4, P5, P6, P7, P8);
impl_scan_pins!(9, P1, P2, P3, P4, P5, P6, P7, P8, P9);
impl_scan_pins!(10, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10);
impl_scan_pins!(11, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11);
impl_scan_pins!(12, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12);
impl_scan_pins!(13, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13);
impl_scan_pins!(14, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14);

/// A scan over an ordered set of channels, with the block's DMA engine storing
/// one 16-bit sample per conversion.
///
/// The sequence is programmed once when the scan is created. Each
/// [`Scan::read`] fills a fresh buffer with whole passes over that sequence.
/// The DMA address registers hold halfword offsets, so a sample count maps
/// directly onto the sequence position.
pub struct Scan<'d, T: Instance, P: ScanPins<T>> {
    /// Keeps exclusive ownership of the pads for the scan's lifetime.
    _pins: P,
    channels: [u8; MAX_CHANNELS],
    _phantom: PhantomData<&'d mut T>,
}

impl<'d, T: Instance, P: ScanPins<T>> Scan<'d, T, P> {
    /// Take the pins and configure the converter for a scan sequence.
    ///
    /// Every pin is floated with its digital input disconnected. The first
    /// conversion after power-up is discarded, as in [`Adc::new_pin`].
    pub fn new(peri: Peri<'d, T>, pins: P, config: Config) -> Self {
        let mut channels = [0; MAX_CHANNELS];
        P::channels(&mut channels);
        for &channel in channels.iter().take(P::COUNT) {
            prepare_pin(channel);
            disconnect(channel);
        }
        enable::<T>(channels[0], config, false);
        // The vendor scan setup lengthens the sample window to seven ADC clocks.
        T::regs().convert().modify(|w| w.set_sample_time(3));
        // Discard the first conversion before the scan source is selected, so
        // it cannot start a pass while the DMA engine is still unarmed.
        let _ = convert_once::<T>();
        let _ = peri;

        let scan = Self {
            _pins: pins,
            channels,
            _phantom: PhantomData,
        };
        scan.configure_sequence();
        scan
    }

    /// Fill `buffer` with whole passes over the sequence.
    ///
    /// `buffer` is exclusive static storage. Its length must be a non-zero
    /// multiple of the sequence length. The returned transfer must be waited on
    /// or stopped before the samples are read.
    pub fn read(
        &mut self,
        buffer: &'static mut [u16],
    ) -> Result<ScanTransfer<'_, 'd, T, P>, (Error, &'static mut [u16])> {
        if buffer.is_empty() || buffer.len() % P::COUNT != 0 {
            return Err((Error::Buffer, buffer));
        }
        let Some(bytes) = buffer.len().checked_mul(2) else {
            return Err((Error::Buffer, buffer));
        };
        let descriptor = match Descriptor::bytes(buffer.as_ptr() as usize, bytes) {
            Ok(descriptor) => descriptor,
            Err(_) => return Err((Error::Buffer, buffer)),
        };

        let one_pass = buffer.len() == P::COUNT;
        let mut transfer = ScanTransfer {
            scan: self,
            buffer: Some(buffer),
            descriptor,
            one_pass,
            finished: false,
        };
        transfer.start();
        Ok(transfer)
    }

    fn configure_sequence(&self) {
        let regs = T::regs();
        let channels = &self.channels;
        regs.channel().write(|w| w.set_ch_inx(channels[0]));
        // `CHANNEL` holds the first entry, then `SCAN_CFG1` takes the next
        // eight and `SCAN_CFG2` the remaining five. Entries beyond the selected
        // count are written too, and `SCAN_NUM` hides them.
        regs.scan_cfg1().write(|w| {
            for entry in 0..SCAN_CFG1_ENTRIES {
                w.set_ch(entry, channels[1 + entry]);
            }
        });
        regs.scan_cfg2().write(|w| {
            for entry in 0..SCAN_CFG2_ENTRIES {
                w.set_ch(entry, channels[1 + SCAN_CFG1_ENTRIES + entry]);
            }
            w.set_scan_num((P::COUNT - 1) as u8);
            w.set_scan_sel(true);
        });
    }
}

impl<T: Instance, P: ScanPins<T>> Drop for Scan<'_, T, P> {
    fn drop(&mut self) {
        let regs = T::regs();
        regs.convert().modify(|w| w.set_start(false));
        regs.ctrl_dma().modify(|w| {
            w.set_dma_enable(false);
            w.set_scan_auto_type(false);
            w.set_ie_dma_end(false);
            w.set_auto_en(false);
        });
        regs.scan_cfg2().modify(|w| {
            w.set_ie_scan_end(false);
            w.set_if_scan_end(true);
            w.set_scan_sel(false);
        });
        regs.dma_if().write(|w| {
            w.set_if_dma_end(true);
            w.set_if_end(true);
        });
        regs.cfg().modify(|w| w.set_power_on(false));
        for &channel in self.channels.iter().take(P::COUNT) {
            reconnect_channel(channel);
        }
    }
}

/// A running ADC scan and DMA transfer.
///
/// Completion detects a full DMA buffer. When the buffer holds exactly one
/// pass, the scan-end flag is also accepted, matching the vendor scan example.
#[must_use = "dropping stops DMA; use blocking_wait or stop"]
pub struct ScanTransfer<'a, 'd, T: Instance, P: ScanPins<T>> {
    scan: &'a mut Scan<'d, T, P>,
    buffer: Option<&'static mut [u16]>,
    descriptor: Descriptor,
    one_pass: bool,
    finished: bool,
}

impl<'a, 'd, T: Instance, P: ScanPins<T>> ScanTransfer<'a, 'd, T, P> {
    /// Samples stored so far, counted from `DMA_NOW`.
    pub fn position(&self) -> usize {
        let now = u32::from(T::regs().dma_now().read().dma_now()) << 1;
        now.saturating_sub(self.descriptor.start) as usize / 2
    }

    /// Every sample in the buffer has been stored.
    pub fn is_complete(&self) -> bool {
        let regs = T::regs();
        regs.dma_if().read().if_dma_end() || (self.one_pass && regs.scan_cfg2().read().if_scan_end())
    }

    /// Wait for the buffer to fill, then stop the scan engine.
    ///
    /// The timeout is one millisecond per sample, which is far above the
    /// conversion rate even at the slowest ADC clock.
    pub fn blocking_wait(mut self) -> Result<&'static mut [u16], (Error, &'static mut [u16])> {
        let timeout = self.samples() as u64 + 1;
        let deadline = deadline_in(timeout);
        while !self.is_complete() {
            if self.continue_pass() {
                continue;
            }
            if timed_out(deadline) {
                self.quiesce();
                return Err((Error::Timeout, self.buffer.take().unwrap()));
            }
            core::hint::spin_loop();
        }
        self.quiesce();
        Ok(self.buffer.take().unwrap())
    }

    /// Stop the engine and hand the buffer back with `position()` samples valid.
    pub fn stop(mut self) -> &'static mut [u16] {
        self.quiesce();
        self.buffer.take().unwrap()
    }

    fn samples(&self) -> usize {
        self.descriptor.len() / 2
    }

    /// Start the next pass after a scan-end event.
    ///
    /// `SCAN_AUTO_TYPE` describes the trigger source; it does not make the
    /// sequencer repeat. One pass ends after the last channel and the next one
    /// needs a fresh `CONVERT.START`. Returns whether a pass ended, even when
    /// the buffer is already full and no new pass was started.
    fn continue_pass(&self) -> bool {
        let regs = T::regs();
        let ended = regs.scan_cfg2().read().if_scan_end();
        if !ended {
            return false;
        }
        regs.scan_cfg2().modify(|w| w.set_if_scan_end(true));
        // The final pass is left alone: the DMA engine has reached END and
        // raises `IF_DMA_END` on its own.
        if !regs.dma_if().read().if_dma_end() {
            regs.convert().modify(|w| w.set_start(true));
        }
        true
    }

    fn start(&mut self) {
        let regs = T::regs();
        // Stop whatever the previous scan left behind before moving END and
        // reprogramming the sequence.
        regs.convert().modify(|w| w.set_start(false));
        regs.ctrl_dma().modify(|w| {
            w.set_dma_enable(false);
            w.set_scan_auto_type(false);
            w.set_ie_dma_end(false);
        });
        regs.dma_if().write(|w| {
            w.set_if_dma_end(true);
            w.set_if_end(true);
        });
        self.scan.configure_sequence();

        // Both registers hold halfword offsets, so the byte descriptor is
        // shifted right by one.
        regs.dma_beg()
            .write(|w| w.set_dma_beg((self.descriptor.start >> 1) as u16));
        regs.dma_end()
            .write(|w| w.set_dma_end((self.descriptor.end >> 1) as u16));
        // `IF_DMA_END` is gated by `IE_DMA_END`, as on the timer and SPI engines.
        regs.ctrl_dma().modify(|w| {
            w.set_dma_enable(true);
            w.set_scan_auto_type(true);
            w.set_ie_dma_end(true);
        });
        // Every pass sets `IF_SCAN_END`; the wait loop uses it to start the next
        // pass. It sets without `IE_SCAN_END`, which stays off so a TouchKey user
        // of the shared line gets no scan interrupts.
        crate::dma::fence();
        regs.convert().modify(|w| w.set_start(true));
    }

    fn quiesce(&mut self) {
        let regs = T::regs();
        regs.convert().modify(|w| w.set_start(false));
        regs.ctrl_dma().modify(|w| {
            w.set_dma_enable(false);
            w.set_scan_auto_type(false);
            w.set_ie_dma_end(false);
        });
        regs.scan_cfg2().modify(|w| {
            w.set_ie_scan_end(false);
            w.set_if_scan_end(true);
        });
        regs.dma_if().write(|w| {
            w.set_if_dma_end(true);
            w.set_if_end(true);
        });
        self.finished = true;
    }
}

impl<T: Instance, P: ScanPins<T>> Drop for ScanTransfer<'_, '_, T, P> {
    fn drop(&mut self) {
        if !self.finished {
            self.quiesce();
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
    let Some((port, pin)) = channel_pin(channel) else {
        return;
    };
    pac::SYS.pin_in_dis().modify(|w| {
        if port == 0 {
            w.set_pa_in_dis(usize::from(pin), true);
        } else {
            w.set_pbl_in_dis(usize::from(pin), true);
        }
    });
}

/// Reconnect the digital input of the pin a channel borrowed.
fn reconnect(port: u8, pin: u8) {
    pac::SYS.pin_in_dis().modify(|w| {
        if port == 0 {
            w.set_pa_in_dis(usize::from(pin), false);
        } else {
            w.set_pbl_in_dis(usize::from(pin), false);
        }
    });
}

fn reconnect_channel(channel: u8) {
    if let Some((port, pin)) = channel_pin(channel) {
        reconnect(port, pin);
    }
}

/// Configure a scan channel as a floating input, which is what the vendor scan
/// setup does before disabling the digital input.
fn prepare_pin(channel: u8) {
    let Some((0, pin)) = channel_pin(channel) else {
        return;
    };
    let gpio = pac::GPIO;
    gpio.pa_dir().modify(|w| w.set_pa_dir(pin as usize, false));
    gpio.pa_pu().modify(|w| w.set_pa_pu(pin as usize, false));
    gpio.pa_pd_drv().modify(|w| w.set_pa_pd_drv(pin as usize, false));
}

/// The pad behind an ADC channel. All external channels are on port A.
fn channel_pin(channel: u8) -> Option<(u8, u8)> {
    // Datasheet table 7-8: A0 is PA4, then PA5, PA12, PA13, PA14, PA15, PA3,
    // PA2, PA1, PA0, PA6, PA7, PA8 and PA9.
    const PINS: [u8; MAX_CHANNELS] = [4, 5, 12, 13, 14, 15, 3, 2, 1, 0, 6, 7, 8, 9];
    PINS.get(channel as usize).map(|&pin| (0, pin))
}

/// One millisecond from now, in system-clock cycles.
fn deadline() -> u64 {
    deadline_in(1)
}

/// `millis` milliseconds from now, in system-clock cycles.
fn deadline_in(millis: u64) -> u64 {
    cycles() + u64::from(crate::rcc::clocks().sysclk.0) / 1000 * millis
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

    pub(crate) trait Instance {
        fn regs() -> pac::adc::Adc;
    }
}

/// An ADC with the CH585 register layout.
#[allow(private_bounds)]
pub trait Instance: sealed::Instance + PeripheralType {
    /// Interrupt line of the ADC block. End-of-conversion asserts it; the scan
    /// and DMA end flags share it, as does the touch-key block.
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
