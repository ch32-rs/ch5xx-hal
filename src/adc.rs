//! ADC conversions and scans.
//!
//! CH585 has one converter, clocked from its own divider rather than `Fsys`.
//! [`Adc`] is a controller handle: [`Adc::blocking_read`] takes the pin or
//! internal source ([`AdcChannel`]) to convert, as in `embassy-stm32`. [`Scan`]
//! samples an ordered sequence of channels and stores the samples with the
//! block's local DMA engine; one pass can cover up to fourteen channels on
//! CH585 and twelve on CH592. [`Adc::start_auto`] instead repeats one channel
//! at a fixed interval, with the same DMA engine filling the buffer. Touch-key
//! mode shares the same converter and is left untouched.
//!
//! Scan and auto-DMA completion is polled from the transfer objects. The ADC
//! interrupt line is asserted by end-of-conversion as well as by the scan and
//! DMA end flags, and the documented `IE_*` bits do not mask that source, so an
//! interrupt-driven wait would spend its time in a handler that cannot clear the
//! line without disturbing the sequence. That is why this module has no
//! interrupt or async read path. A TouchKey driver taking over the line must
//! account for this.
//!
//! A channel has to be selected before the converter is powered, which is the
//! order the vendor setup uses. On CH585 an external channel also disconnects
//! the pad's digital input through `PIN_IN_DIS`, or the pin driver fights the
//! signal being measured; CH592 has no such register and only floats the pad.
//! Each read powers the converter up and down and restores the pad.
//!
//! The raw result is 12 bits. The vendor notes an offset near the ends of the
//! range; [`Adc::blocking_read`] returns the sample unchanged, and
//! [`Adc::calibrate`] measures that offset so a caller can add it.
//!
//! The register block differs between dies. The module compiles for the CH585
//! layout (CH581, CH584, CH585) and for CH592. CH592 has three `SCAN_CFG2`
//! entries instead of five, no `SAMPLE_TIME` and no `SCAN_AUTO_TYPE`; it also
//! routes AIN8/AIN9 to PB0/PB6 and has no AIN6/AIN7. Both dies have been run on
//! a board. Other dies have no driver.

use core::marker::PhantomData;

use embassy_hal_internal::PeripheralType;

use crate::dma::Descriptor;
use crate::gpio::Pin;
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

mod channel_sealed {
    use super::{disconnect_pad, prepare_pad, reconnect_pad, release_pad, Config, Instance};

    pub trait Channel<T: Instance> {
        /// The hardware channel index.
        fn index() -> u8;
        /// The pad behind an external channel, as `(port, pin)`.
        fn pad() -> Option<(u8, u8)> {
            None
        }
        /// The converter configuration this channel needs, and whether the
        /// conversion is differential.
        fn config(&self, base: Config) -> (Config, bool) {
            (base, false)
        }
        /// Float the pad and cut its digital input before the converter powers.
        fn prepare() {
            if let Some((port, pin)) = Self::pad() {
                prepare_pad(port, pin);
                disconnect_pad(port, pin);
            }
        }
        /// Power the converter down and restore the pad.
        fn release() {
            release_pad::<T>(Self::pad());
        }
        /// Restore the pad without touching the converter.
        fn restore() {
            if let Some((port, pin)) = Self::pad() {
                reconnect_pad(port, pin);
            }
        }
    }
}

/// A pin or internal source the ADC can convert.
///
/// Implemented for ADC pins (`Peri<'_, PA4>` and friends) and for the
/// [`Temperature`] and [`Battery`] tokens. Pass one to [`Adc::blocking_read`]
/// or [`Adc::start_auto`].
#[allow(private_bounds)]
pub trait AdcChannel<T: Instance>: channel_sealed::Channel<T> {}

/// The internal temperature sensor, from [`Adc::enable_temperature`].
///
/// Dropping it powers the sensor down. It overrides the controller's gain and
/// clock: the vendor setup forces differential mode and a gain of two.
pub struct Temperature {
    _private: (),
}

impl channel_sealed::Channel<peripherals::ADC> for Temperature {
    fn index() -> u8 {
        TEMPERATURE_CHANNEL
    }

    fn config(&self, _base: Config) -> (Config, bool) {
        (temperature_config(), true)
    }
}

impl AdcChannel<peripherals::ADC> for Temperature {}

impl Drop for Temperature {
    fn drop(&mut self) {
        pac::ADC.tem_sensor().write(|w| w.set_tem_sen_pwr_on(false));
    }
}

/// The divided battery-voltage channel.
///
/// Nothing needs powering on, so the token is built directly and can be used
/// with [`Adc::blocking_read`] or put in a [`Scan`] sequence. The vendor setup
/// uses a gain of one quarter and the input buffer, since the battery is
/// divided down to the converter's range; pass such a [`Config`] to
/// [`Adc::new`].
pub struct Battery;

impl channel_sealed::Channel<peripherals::ADC> for Battery {
    fn index() -> u8 {
        BATTERY_CHANNEL
    }
}

impl AdcChannel<peripherals::ADC> for Battery {}

/// A single-conversion ADC controller.
///
/// The controller owns the converter and carries the gain and clock for
/// external channels; the channel is chosen per call, as in `embassy-stm32`.
pub struct Adc<'d, T: Instance> {
    _peri: Peri<'d, T>,
    config: Config,
}

impl<'d, T: Instance> Adc<'d, T> {
    /// Take the converter and set the external-channel configuration.
    pub fn new(peri: Peri<'d, T>, config: Config) -> Self {
        Self { _peri: peri, config }
    }

    /// One conversion of `channel`, as a raw 12-bit sample.
    ///
    /// The pad is floated and its digital input disconnected before the
    /// converter powers up, and restored afterwards. The first conversion
    /// after power-up is stale; as in the vendor flow, the sample returned is
    /// the second one.
    pub fn blocking_read<C: AdcChannel<T>>(&mut self, channel: &mut C) -> Result<u16, Error> {
        self.begin(channel);
        let result = convert_twice::<T>();
        <C as channel_sealed::Channel<T>>::release();
        result
    }

    /// Float the pad, cut its digital input and power the converter up.
    fn begin<C: AdcChannel<T>>(&self, channel: &C) {
        let index = <C as channel_sealed::Channel<T>>::index();
        let (config, differential) = channel.config(self.config);
        <C as channel_sealed::Channel<T>>::prepare();
        enable::<T>(index, config, differential);
    }

    /// Power the internal temperature sensor and return its channel.
    ///
    /// [`Adc::temperature`] turns a raw sample into a temperature with the
    /// value stored in the info flash.
    pub fn enable_temperature(&mut self) -> Temperature {
        T::regs().tem_sensor().write(|w| w.set_tem_sen_pwr_on(true));
        Temperature { _private: () }
    }

    /// The converter's own offset, from the vendor's rough calibration.
    ///
    /// Sixteen conversions of the shorted input are averaged and subtracted
    /// from the mid-scale value. Add the result to a [`Adc::blocking_read`]
    /// sample. The converter's previous configuration is restored afterwards.
    pub fn calibrate(&mut self) -> Result<i16, Error> {
        let regs = T::regs();
        let saved = regs.cfg().read();
        let unity = Config {
            gain: Gain::Unity,
            sample_clock: self.config.sample_clock,
        };
        // The single-ended unity-gain setup also turns the input buffer on,
        // which the shorted input needs.
        enable::<T>(regs.channel().read().ch_inx(), unity, false);
        regs.cfg().modify(|w| w.set_ofs_test(true));
        let result = calibrate_offset::<T>();
        regs.cfg().write_value(saved);
        result
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
}

/// Two conversions; the first one after power-up is stale, so it is discarded.
fn convert_twice<T: Instance>() -> Result<u16, Error> {
    let _ = convert_once::<T>()?;
    convert_once::<T>()
}

/// The vendor's rough offset calibration, with the converter already powered.
fn calibrate_offset<T: Instance>() -> Result<i16, Error> {
    let _ = convert_once::<T>()?;
    let mut sum = 0u32;
    for _ in 0..16 {
        // The offset test reports the complement of the data bits.
        sum += u32::from(!convert_once::<T>()? & DATA_MASK);
    }
    Ok(2048 - ((sum + 8) >> 4) as i16)
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

// --- scan and local DMA ----------------------------------------------------

/// Sequence entries in `SCAN_CFG1`, after the one held by `CHANNEL`.
#[cfg(adc_scan)]
const SCAN_CFG1_ENTRIES: usize = 8;

/// Sequence entries in `SCAN_CFG2`, after `SCAN_CFG1`.
#[cfg(all(adc_scan, adc_ch585))]
const SCAN_CFG2_ENTRIES: usize = 5;

/// Sequence entries in `SCAN_CFG2`, after `SCAN_CFG1`.
#[cfg(all(adc_scan, adc_ch592))]
const SCAN_CFG2_ENTRIES: usize = 3;

/// The largest scan sequence the sequencer accepts: `CHANNEL` plus the two
/// sequence registers.
#[cfg(adc_scan)]
pub const MAX_CHANNELS: usize = 1 + SCAN_CFG1_ENTRIES + SCAN_CFG2_ENTRIES;

#[cfg(adc_scan)]
mod scan_sealed {
    use super::{Instance, MAX_CHANNELS};

    pub(crate) trait Sequence<T: Instance> {
        const COUNT: usize;
        fn channels(sequence: &mut [u8; MAX_CHANNELS]);
        fn prepare();
        fn release();
    }
}

/// One entry in a scan sequence: an ADC pin or the [`Battery`] channel.
///
/// [`Temperature`] is not a sequence entry: it forces differential mode, which
/// would apply to the whole pass, so it is only usable with
/// [`Adc::blocking_read`] or [`Adc::start_auto`].
#[cfg(adc_scan)]
#[allow(private_bounds)]
pub trait ScanEntry<T: Instance>: channel_sealed::Channel<T> {}

#[cfg(adc_scan)]
impl<'d, T: Instance, P: AdcPin<T>> ScanEntry<T> for Peri<'d, P> {}

#[cfg(adc_scan)]
impl ScanEntry<peripherals::ADC> for Battery {}

/// A tuple of channels, scanned in tuple order.
///
/// Tuples of one to [`MAX_CHANNELS`] entries are supported; an entry is an ADC
/// pin or the [`Battery`] channel. The order of the tuple is the order of the
/// sequence: the first element comes from `CHANNEL.CH_INX`, the next eight from
/// `SCAN_CFG1` and the rest from `SCAN_CFG2`. Pins are configured as floating
/// analog inputs when the [`Scan`] is created, and their digital inputs are
/// reconnected when it is dropped.
#[cfg(adc_scan)]
#[allow(private_bounds)]
pub trait ScanChannels<T: Instance>: scan_sealed::Sequence<T> {}

#[cfg(adc_scan)]
macro_rules! impl_scan_channels {
    ($count:literal, $($ch:ident),+) => {
        impl<T: Instance, $($ch: ScanEntry<T>),+> scan_sealed::Sequence<T> for ($($ch,)+) {
            const COUNT: usize = $count;

            fn channels(sequence: &mut [u8; MAX_CHANNELS]) {
                let entries = [$(
                    <$ch as channel_sealed::Channel<T>>::index(),
                )+];
                sequence[..$count].copy_from_slice(&entries);
            }

            fn prepare() {
                $(<$ch as channel_sealed::Channel<T>>::prepare();)+
            }

            fn release() {
                T::regs().cfg().modify(|w| w.set_power_on(false));
                $(<$ch as channel_sealed::Channel<T>>::restore();)+
            }
        }

        impl<T: Instance, $($ch: ScanEntry<T>),+> ScanChannels<T> for ($($ch,)+) {}
    };
}

#[cfg(adc_scan)]
impl_scan_channels!(1, C1);
#[cfg(adc_scan)]
impl_scan_channels!(2, C1, C2);
#[cfg(adc_scan)]
impl_scan_channels!(3, C1, C2, C3);
#[cfg(adc_scan)]
impl_scan_channels!(4, C1, C2, C3, C4);
#[cfg(adc_scan)]
impl_scan_channels!(5, C1, C2, C3, C4, C5);
#[cfg(adc_scan)]
impl_scan_channels!(6, C1, C2, C3, C4, C5, C6);
#[cfg(adc_scan)]
impl_scan_channels!(7, C1, C2, C3, C4, C5, C6, C7);
#[cfg(adc_scan)]
impl_scan_channels!(8, C1, C2, C3, C4, C5, C6, C7, C8);
#[cfg(adc_scan)]
impl_scan_channels!(9, C1, C2, C3, C4, C5, C6, C7, C8, C9);
#[cfg(adc_scan)]
impl_scan_channels!(10, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10);
#[cfg(adc_scan)]
impl_scan_channels!(11, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11);
#[cfg(adc_scan)]
impl_scan_channels!(12, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C12);
#[cfg(all(adc_scan, adc_ch585))]
impl_scan_channels!(13, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C12, C13);
#[cfg(all(adc_scan, adc_ch585))]
impl_scan_channels!(14, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C12, C13, C14);

/// A scan over an ordered set of channels, with the block's DMA engine storing
/// one 16-bit sample per conversion.
///
/// The sequence is programmed once when the scan is created. Each
/// [`Scan::read`] fills a fresh buffer with whole passes over that sequence.
/// The DMA address registers hold halfword offsets, so a sample count maps
/// directly onto the sequence position.
#[cfg(adc_scan)]
pub struct Scan<'d, T: Instance, P: ScanChannels<T>> {
    /// Keeps exclusive ownership of the channels for the scan's lifetime.
    _channels: P,
    channels: [u8; MAX_CHANNELS],
    _phantom: PhantomData<&'d mut T>,
}

#[cfg(adc_scan)]
impl<'d, T: Instance, P: ScanChannels<T>> Scan<'d, T, P> {
    /// Take the channels and configure the converter for a scan sequence.
    ///
    /// Every pin is floated with its digital input disconnected. The first
    /// conversion after power-up is discarded, as in [`Adc::blocking_read`].
    pub fn new(peri: Peri<'d, T>, channels: P, config: Config) -> Self {
        let mut sequence = [0; MAX_CHANNELS];
        P::channels(&mut sequence);
        P::prepare();
        enable::<T>(sequence[0], config, false);
        // The vendor scan setup lengthens the sample window to seven ADC clocks.
        // CH592 has no sample-time field; its window is fixed.
        #[cfg(adc_ch585)]
        T::regs().convert().modify(|w| w.set_sample_time(3));
        // Discard the first conversion before the scan source is selected, so
        // it cannot start a pass while the DMA engine is still unarmed.
        let _ = convert_once::<T>();
        let _ = peri;

        let scan = Self {
            _channels: channels,
            channels: sequence,
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

#[cfg(adc_scan)]
impl<T: Instance, P: ScanChannels<T>> Drop for Scan<'_, T, P> {
    fn drop(&mut self) {
        let regs = T::regs();
        regs.convert().modify(|w| w.set_start(false));
        regs.ctrl_dma().modify(|w| {
            w.set_dma_enable(false);
            #[cfg(adc_ch585)]
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
        P::release();
    }
}

/// A running ADC scan and DMA transfer.
///
/// Completion detects a full DMA buffer. When the buffer holds exactly one
/// pass, the scan-end flag is also accepted, matching the vendor scan example.
#[must_use = "dropping stops DMA; use blocking_wait or stop"]
#[cfg(adc_scan)]
pub struct ScanTransfer<'a, 'd, T: Instance, P: ScanChannels<T>> {
    scan: &'a mut Scan<'d, T, P>,
    buffer: Option<&'static mut [u16]>,
    descriptor: Descriptor,
    one_pass: bool,
    finished: bool,
}

#[cfg(adc_scan)]
impl<'a, 'd, T: Instance, P: ScanChannels<T>> ScanTransfer<'a, 'd, T, P> {
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
            // CH592 has no `SCAN_AUTO_TYPE`; this driver starts every pass with
            // `CONVERT.START` on both dies.
            #[cfg(adc_ch585)]
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
            #[cfg(adc_ch585)]
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
            #[cfg(adc_ch585)]
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

#[cfg(adc_scan)]
impl<T: Instance, P: ScanChannels<T>> Drop for ScanTransfer<'_, '_, T, P> {
    fn drop(&mut self) {
        if !self.finished {
            self.quiesce();
        }
    }
}

// --- timed auto sampling ---------------------------------------------------

/// Period between automatic conversions.
///
/// The hardware converts every `(256 - value) * 16` system clocks, so a larger
/// register value is a shorter period. At the 6.4 MHz reset clock the range is
/// 2.5 us to 640 us.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AutoCycle(u8);

impl AutoCycle {
    /// The longest period, 4096 system clocks.
    pub const LONGEST: Self = Self(0);
    /// The shortest period, 16 system clocks.
    pub const SHORTEST: Self = Self(255);

    /// A raw `AUTO_CYCLE` register value.
    pub const fn raw(value: u8) -> Self {
        Self(value)
    }

    /// The closest period that is not longer than `clocks` system clocks.
    pub const fn from_clocks(clocks: u32) -> Self {
        let units = clocks / 16;
        if units >= 256 {
            Self::LONGEST
        } else if units == 0 {
            Self::SHORTEST
        } else {
            Self((256 - units) as u8)
        }
    }

    /// The programmed period in system clocks.
    pub const fn clocks(self) -> u32 {
        (256 - self.0 as u32) * 16
    }
}

impl<'d, T: Instance> Adc<'d, T> {
    /// Sample `channel` repeatedly into `buffer`.
    ///
    /// The converter converts once every `cycle` and the block's DMA engine
    /// stores one 16-bit sample per conversion, filling `buffer` in order. The
    /// sequence in [`Scan`] is not involved: this is the single-channel timed
    /// mode of datasheet section 15.3.3. `buffer` is exclusive static storage
    /// and the transfer must be waited on or stopped before it is read.
    pub fn start_auto<C: AdcChannel<T>>(
        &mut self,
        channel: &mut C,
        buffer: &'static mut [u16],
        cycle: AutoCycle,
    ) -> Result<AutoTransfer<'_, 'd, T>, (Error, &'static mut [u16])> {
        if buffer.is_empty() {
            return Err((Error::Buffer, buffer));
        }
        let Some(bytes) = buffer.len().checked_mul(2) else {
            return Err((Error::Buffer, buffer));
        };
        let descriptor = match Descriptor::bytes(buffer.as_ptr() as usize, bytes) {
            Ok(descriptor) => descriptor,
            Err(_) => return Err((Error::Buffer, buffer)),
        };

        self.begin(channel);

        let regs = T::regs();
        // Stop a previous user of the engine before moving the buffer window.
        regs.convert().modify(|w| w.set_start(false));
        regs.ctrl_dma().modify(|w| {
            w.set_auto_en(false);
            w.set_dma_enable(false);
            w.set_ie_dma_end(false);
        });
        regs.dma_if().write(|w| {
            w.set_if_dma_end(true);
            w.set_if_end(true);
        });
        // Both address registers hold halfword offsets.
        regs.dma_beg().write(|w| w.set_dma_beg((descriptor.start >> 1) as u16));
        regs.dma_end().write(|w| w.set_dma_end((descriptor.end >> 1) as u16));
        regs.auto_cycle().write(|w| w.set_auto_cycle(cycle.0));
        crate::dma::fence();
        // The datasheet sequence starts timed sampling with `AUTO_EN`; it does
        // not ask for a `CONVERT.START` write.
        regs.ctrl_dma().modify(|w| {
            w.set_dma_enable(true);
            w.set_ie_dma_end(true);
            w.set_auto_en(true);
        });

        Ok(AutoTransfer {
            _adc: self,
            buffer: Some(buffer),
            descriptor,
            pad: <C as channel_sealed::Channel<T>>::pad(),
            finished: false,
        })
    }
}

/// A running timed-sampling transfer.
///
/// Created by [`Adc::start_auto`]. Completion means the DMA engine reached the
/// end of the buffer; the converter keeps sampling until the transfer is
/// stopped.
#[must_use = "dropping stops auto sampling; use blocking_wait or stop"]
pub struct AutoTransfer<'a, 'd, T: Instance> {
    /// Keeps the converter borrowed while the engine is running.
    _adc: &'a mut Adc<'d, T>,
    buffer: Option<&'static mut [u16]>,
    descriptor: Descriptor,
    /// Pad behind the channel, so it is restored when the transfer stops.
    pad: Option<(u8, u8)>,
    finished: bool,
}

impl<'a, 'd, T: Instance> AutoTransfer<'a, 'd, T> {
    /// Samples stored so far, counted from `DMA_NOW`.
    pub fn position(&self) -> usize {
        let now = u32::from(T::regs().dma_now().read().dma_now()) << 1;
        now.saturating_sub(self.descriptor.start) as usize / 2
    }

    /// Every sample in the buffer has been stored.
    pub fn is_complete(&self) -> bool {
        T::regs().dma_if().read().if_dma_end()
    }

    /// Wait for the buffer to fill, then stop the converter.
    ///
    /// The timeout is one millisecond per sample, which covers the longest
    /// programmed period at the reset clock.
    pub fn blocking_wait(mut self) -> Result<&'static mut [u16], (Error, &'static mut [u16])> {
        let timeout = self.descriptor.len() as u64 / 2 + 1;
        let deadline = deadline_in(timeout);
        while !self.is_complete() {
            if timed_out(deadline) {
                self.quiesce();
                return Err((Error::Timeout, self.buffer.take().unwrap()));
            }
            core::hint::spin_loop();
        }
        self.quiesce();
        Ok(self.buffer.take().unwrap())
    }

    /// Stop the converter and hand the buffer back with `position()` samples
    /// valid.
    pub fn stop(mut self) -> &'static mut [u16] {
        self.quiesce();
        self.buffer.take().unwrap()
    }

    fn quiesce(&mut self) {
        let regs = T::regs();
        regs.ctrl_dma().modify(|w| {
            w.set_auto_en(false);
            w.set_dma_enable(false);
            w.set_ie_dma_end(false);
        });
        regs.convert().modify(|w| w.set_start(false));
        regs.dma_if().write(|w| {
            w.set_if_dma_end(true);
            w.set_if_end(true);
        });
        release_pad::<T>(self.pad);
        crate::dma::fence();
        self.finished = true;
    }
}

impl<T: Instance> Drop for AutoTransfer<'_, '_, T> {
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
pub(crate) fn enable<T: Instance>(channel: u8, config: Config, differential: bool) {
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

/// Float a pad, so the pin driver does not fight the signal being measured.
fn prepare_pad(port: u8, pin: u8) {
    let gpio = pac::GPIO;
    let pin = pin as usize;
    if port == 0 {
        gpio.pa_dir().modify(|w| w.set_pa_dir(pin, false));
        gpio.pa_pu().modify(|w| w.set_pa_pu(pin, false));
        gpio.pa_pd_drv().modify(|w| w.set_pa_pd_drv(pin, false));
    } else {
        gpio.pb_dir().modify(|w| w.set_pb_dir(pin, false));
        gpio.pb_pu().modify(|w| w.set_pb_pu(pin, false));
        gpio.pb_pd_drv().modify(|w| w.set_pb_pd_drv(pin, false));
    }
}

/// Disconnect the digital input of an ADC pin, so the pad driver is out of the way.
///
/// CH585 removes the pad's digital input through `PIN_IN_DIS`. CH592 has no
/// such register and only floats the pad, which [`prepare_pad`] already does.
#[cfg(adc_ch585)]
fn disconnect_pad(port: u8, pin: u8) {
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

#[cfg(not(adc_ch585))]
fn disconnect_pad(_port: u8, _pin: u8) {}

/// Reconnect the digital input of the pin a channel borrowed.
///
/// Only the CH585 layout gates the pad's digital input, so this is a no-op
/// elsewhere.
#[cfg(adc_ch585)]
fn reconnect_pad(port: u8, pin: u8) {
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

#[cfg(not(adc_ch585))]
fn reconnect_pad(_port: u8, _pin: u8) {}

/// Power the converter down and restore a channel's pad.
fn release_pad<T: Instance>(pad: Option<(u8, u8)>) {
    T::regs().cfg().modify(|w| w.set_power_on(false));
    if let Some((port, pin)) = pad {
        reconnect_pad(port, pin);
    }
}

/// One millisecond from now, in system-clock cycles.
fn deadline() -> u64 {
    deadline_in(1)
}

/// `millis` milliseconds from now, in system-clock cycles.
pub(crate) fn deadline_in(millis: u64) -> u64 {
    cycles() + u64::from(crate::rcc::clocks().sysclk.0) / 1000 * millis
}

pub(crate) fn timed_out(deadline: u64) -> bool {
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

/// The ADC instance.
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
        /// The channel index and the pad bit this pin measures.
        fn index() -> (u8, u8);
        /// The pad behind the pin, as `(port, pin)`.
        fn pad() -> (u8, u8);
    }
}

/// A pin that can be an ADC input on `T`.
pub trait AdcPin<T: Instance>: Pin + pin_sealed::Channel {
    /// The channel index and the pad bit this pin measures.
    fn channel() -> (u8, u8) {
        <Self as pin_sealed::Channel>::index()
    }
}

impl<'d, T: Instance, P: AdcPin<T>> channel_sealed::Channel<T> for Peri<'d, P> {
    fn index() -> u8 {
        <P as pin_sealed::Channel>::index().0
    }

    fn pad() -> Option<(u8, u8)> {
        Some(<P as pin_sealed::Channel>::pad())
    }
}

impl<'d, T: Instance, P: AdcPin<T>> AdcChannel<T> for Peri<'d, P> {}

macro_rules! impl_channel {
    ($pin:ident, $channel:literal, $bit:literal, $port:literal) => {
        impl pin_sealed::Channel for peripherals::$pin {
            fn index() -> (u8, u8) {
                ($channel, $bit)
            }

            fn pad() -> (u8, u8) {
                ($port, $bit)
            }
        }
        impl AdcPin<peripherals::ADC> for peripherals::$pin {}
    };
}

// The channel number is the pin's index in the family pin list. CH592 drops
// AIN6/AIN7 (PA3/PA2) and moves AIN8/AIN9 from PA1/PA0 to PB0/PB6. Arguments
// are the pin, its channel, its pad bit and its port.
impl_channel!(PA4, 0, 4, 0);
impl_channel!(PA5, 1, 5, 0);
impl_channel!(PA12, 2, 12, 0);
impl_channel!(PA13, 3, 13, 0);
impl_channel!(PA14, 4, 14, 0);
impl_channel!(PA15, 5, 15, 0);
#[cfg(adc_ch585)]
impl_channel!(PA3, 6, 3, 0);
#[cfg(adc_ch585)]
impl_channel!(PA2, 7, 2, 0);
#[cfg(adc_ch585)]
impl_channel!(PA1, 8, 1, 0);
#[cfg(adc_ch585)]
impl_channel!(PA0, 9, 0, 0);
#[cfg(adc_ch592)]
impl_channel!(PB0, 8, 0, 1);
#[cfg(adc_ch592)]
impl_channel!(PB6, 9, 6, 1);
impl_channel!(PA6, 10, 6, 0);
impl_channel!(PA7, 11, 7, 0);
impl_channel!(PA8, 12, 8, 0);
impl_channel!(PA9, 13, 9, 0);
