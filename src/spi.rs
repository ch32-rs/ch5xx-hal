//! SPI master.
//!
//! CH585 has two controllers, both clocked from `Fsys`. `SPI0` is a full
//! controller with DMA; `SPI1` is master-only and has no DMA block. This driver
//! uses neither DMA nor interrupts. A transfer writes one byte to `FIFO` and
//! reads the byte shifted in from `BUFFER`, then waits for `INT_FLAG.FREE`.
//!
//! `CLOCK_DIV` divides `Fsys` by a whole number. A divisor of 2 turns on the
//! master's input delay, which is what the vendor clock setup does; every other
//! divisor leaves it off. The controller only has mode 0 and mode 3, so a
//! request for mode 1 or mode 2 is rejected.
//!
//! `SPI0` defaults to PA13 (SCK), PA14 (MOSI) and PA15 (MISO). `PIN_SPI0` moves
//! that set to PB13, PB14 and PB15. `SPI1` has no remap: SCK is PA0, MOSI is PA1
//! and MISO is PA2. Chip select is not part of the controller. The vendor
//! examples drive it from GPIO, and so does [`chip_select`].
//!
//! The register block is not shared by every CH5xx. The driver compiles on the
//! dies that use the CH585 layout (CH581, CH582, CH583, CH584, CH585) and is
//! absent on the rest. Only CH585 has been run on a board. `SPI1` raises
//! `PWMX_SPI1`, the same line as the PWM block, so a later async driver has to
//! share that handler rather than own it.

use core::marker::PhantomData;

use embassy_hal_internal::PeripheralType;
use embedded_hal::spi::{Phase, Polarity};

use crate::gpio::{Flex, Level, Pin, Speed};
use crate::mode::{Blocking, Mode};
use crate::time::Hertz;
use crate::{interrupt, pac, peripherals, Peri};

/// Errors a master transfer can return.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Error {
    /// The bus did not go idle within one millisecond.
    Timeout,
    /// `FIFO_COUNT` reported more bytes than the FIFO holds.
    Overrun,
}

/// Why a constructor refused the configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The requested rate is zero, or `Fsys` cannot reach it with a divisor of 2.
    Frequency,
    /// The controller has no mode 1 or mode 2. Only mode 0 and mode 3 exist.
    Mode,
}

impl embedded_hal::spi::Error for Error {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        use embedded_hal::spi::ErrorKind;
        match self {
            Error::Timeout => ErrorKind::Other,
            Error::Overrun => ErrorKind::Overrun,
        }
    }
}

/// Which end of a byte is shifted out first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BitOrder {
    /// Most significant bit first. The controller's reset state.
    MsbFirst,
    /// Least significant bit first.
    LsbFirst,
}

/// Master configuration.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Config {
    /// Bus rate. The driver rounds down to the nearest whole divisor of `Fsys`.
    pub frequency: Hertz,
    /// Clock polarity and phase. Only mode 0 and mode 3 are accepted.
    pub mode: embedded_hal::spi::Mode,
    /// Bit order within each byte.
    pub bit_order: BitOrder,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            frequency: Hertz(1_000_000),
            mode: embedded_hal::spi::MODE_0,
            bit_order: BitOrder::MsbFirst,
        }
    }
}

impl Config {
    /// `frequency` in mode 0, MSB first.
    pub const fn new(frequency: Hertz) -> Self {
        Self {
            frequency,
            mode: embedded_hal::spi::MODE_0,
            bit_order: BitOrder::MsbFirst,
        }
    }
}

/// Clock fields programmed into `CLOCK_DIV` and `CTRL_CFG`.
struct Timing {
    /// `CLOCK_DIV`. `Fsys / divisor` is the SCK rate.
    divisor: u8,
    /// `CTRL_CFG.MST_DLY_EN`, set only for a divisor of 2.
    input_delay: bool,
}

fn timing(input: Hertz, frequency: Hertz) -> Result<Timing, ConfigError> {
    if frequency.0 == 0 || frequency.0 > input.0 / 2 {
        return Err(ConfigError::Frequency);
    }
    // Round down so the bus never runs faster than asked. Divisor 0 and 1 are
    // not clock rates the vendor setup programs.
    let divisor = input.0 / frequency.0;
    if divisor > u8::MAX as u32 {
        return Err(ConfigError::Frequency);
    }
    Ok(Timing {
        divisor: divisor as u8,
        input_delay: divisor == 2,
    })
}

/// Drive SCK to the level it idles at. Mode 0 idles low, mode 3 high.
fn idle_clock(sck: &mut Flex<'_>, mode3: bool) {
    if mode3 {
        sck.set_high();
    } else {
        sck.set_low();
    }
}

/// Which clock edges the controller can produce.
fn clock_mode(mode: embedded_hal::spi::Mode) -> Result<bool, ConfigError> {
    match (mode.polarity, mode.phase) {
        (Polarity::IdleLow, Phase::CaptureOnFirstTransition) => Ok(false),
        (Polarity::IdleHigh, Phase::CaptureOnSecondTransition) => Ok(true),
        _ => Err(ConfigError::Mode),
    }
}

/// Blocking SPI master.
pub struct Spi<'d, T: Instance, M: Mode> {
    sck: Flex<'d>,
    mosi: Flex<'d>,
    miso: Flex<'d>,
    _phantom: PhantomData<(&'d mut T, M)>,
}

impl<'d, T: Instance> Spi<'d, T, Blocking> {
    /// Create a blocking master.
    ///
    /// `REMAP` is `0` for the default pins and `1` for the remapped set. `SPI1`
    /// has no remap, so only `0` compiles. The pins must match the value: a
    /// default pin cannot be passed with the remapped one.
    pub fn new_blocking<const REMAP: u8>(
        _peri: Peri<'d, T>,
        sck: Peri<'d, impl SckPin<T, REMAP>>,
        mosi: Peri<'d, impl MosiPin<T, REMAP>>,
        miso: Peri<'d, impl MisoPin<T, REMAP>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let timing = timing(T::frequency(), config.frequency)?;
        let mode3 = clock_mode(config.mode)?;
        let mut sck = Flex::new(sck.into());
        let mut mosi = Flex::new(mosi.into());
        let mut miso = Flex::new(miso.into());
        // Hold both outputs idle before the controller drives them, so enabling
        // the outputs does not glitch the bus.
        idle_clock(&mut sck, mode3);
        mosi.set_low();
        sck.set_as_output(Speed::High);
        mosi.set_as_output(Speed::High);
        // MISO is sampled by the controller, so the pad stays an input. The
        // pull-up only biases an undriven line.
        miso.set_as_input(crate::gpio::Pull::Up);
        critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });
        apply::<T>(&timing, mode3, config.bit_order);
        Ok(Self {
            sck,
            mosi,
            miso,
            _phantom: PhantomData,
        })
    }
}

impl<'d, T: Instance, M: Mode> Spi<'d, T, M> {
    /// Program a new rate, clock mode or bit order.
    ///
    /// The pins stay as they are. Mode 0 and mode 3 idle SCK at opposite levels,
    /// and the pad is driven to the new idle level before the controller is
    /// reprogrammed.
    pub fn set_config(&mut self, config: Config) -> Result<(), ConfigError> {
        let timing = timing(T::frequency(), config.frequency)?;
        let mode3 = clock_mode(config.mode)?;
        idle_clock(&mut self.sck, mode3);
        apply::<T>(&timing, mode3, config.bit_order);
        Ok(())
    }

    /// Write `bytes`, ignoring what comes back.
    pub fn blocking_write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.transfer(bytes.len(), |i| bytes[i], |_, _| {})
    }

    /// Read `buffer.len()` bytes. The controller clocks `0xFF` out on MOSI.
    pub fn blocking_read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.transfer(buffer.len(), |_| 0xFF, |i, byte| buffer[i] = byte)
    }

    /// Exchange `buffer` in place.
    pub fn blocking_transfer_in_place(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        // The outgoing and incoming closures would both borrow `buffer`, so each
        // piece is staged through a copy the FIFO can hold.
        for chunk in buffer.chunks_mut(FIFO_DEPTH as usize) {
            let mut staged = [0u8; FIFO_DEPTH as usize];
            staged[..chunk.len()].copy_from_slice(chunk);
            self.exchange(chunk.len(), |i| staged[i], |i, byte| chunk[i] = byte)?;
        }
        Ok(())
    }

    /// Write `write` and read into `read` at the same time.
    ///
    /// The transfer runs for the longer slice. Extra bytes read past `read` are
    /// discarded, and once `write` runs out the controller clocks `0xFF`.
    pub fn blocking_transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        let length = read.len().max(write.len());
        self.transfer(
            length,
            |i| write.get(i).copied().unwrap_or(0xFF),
            |i, byte| {
                if let Some(slot) = read.get_mut(i) {
                    *slot = byte;
                }
            },
        )
    }

    fn transfer(
        &mut self,
        length: usize,
        mut outgoing: impl FnMut(usize) -> u8,
        mut incoming: impl FnMut(usize, u8),
    ) -> Result<(), Error> {
        // `TOTAL_CNT` is 12 bits, so a longer buffer goes out in pieces.
        let mut start = 0;
        while start < length {
            let count = (length - start).min(MAX_COUNT);
            self.exchange(count, |i| outgoing(start + i), |i, byte| incoming(start + i, byte))?;
            start += count;
        }
        Ok(())
    }

    fn exchange(
        &mut self,
        count: usize,
        mut outgoing: impl FnMut(usize) -> u8,
        mut incoming: impl FnMut(usize, u8),
    ) -> Result<(), Error> {
        let regs = T::regs();
        // Full duplex stays in the output direction. The vendor flash routine
        // writes `FIFO` and reads the shifted-in byte back from `BUFFER`; setting
        // `FIFO_DIR` makes the FIFO a receive buffer and discards what was sent.
        regs.ctrl_mod().modify(|w| w.set_fifo_dir(false));
        regs.total_cnt().write(|w| w.set_total_cnt(count as u16));
        // The count-end flag is write-1-to-clear and latches from a previous
        // chunk, so drop it before waiting on this one.
        regs.int_flag().write(|w| w.set_if_cnt_end(true));

        let mut sent = 0;
        let deadline = deadline();
        while sent < count {
            // One byte at a time: `BUFFER` holds the last byte shifted in, so
            // reading it before the next write is the only way to keep every byte.
            if fifo_count::<T>()? == 0 {
                regs.fifo().write(|w| w.set_fifo(outgoing(sent)));
                self.wait_idle()?;
                incoming(sent, regs.buffer().read().buffer());
                sent += 1;
            } else if timed_out(deadline) {
                return Err(Error::Timeout);
            }
        }
        Ok(())
    }

    /// Wait until the shifter has finished the byte just queued.
    fn wait_idle(&mut self) -> Result<(), Error> {
        let deadline = deadline();
        loop {
            if fifo_count::<T>()? == 0 && T::regs().int_flag().read().free() {
                return Ok(());
            }
            if timed_out(deadline) {
                return Err(Error::Timeout);
            }
        }
    }
}

/// A GPIO chip select.
///
/// The controller has no CS pin of its own, and the vendor examples drive it from
/// GPIO. `idle` is the level the line rests at; [`ChipSelect::select`] drives the
/// opposite level for the duration of one closure.
pub fn chip_select<'d>(pin: Peri<'d, impl Pin>, idle: Level) -> ChipSelect<'d> {
    ChipSelect {
        pin: crate::gpio::Output::new(pin, idle, Speed::High),
    }
}

/// GPIO chip select for one device.
///
/// The controller does not own this line, so the caller drives it around a
/// transfer.
pub struct ChipSelect<'d> {
    pin: crate::gpio::Output<'d>,
}

impl<'d> ChipSelect<'d> {
    /// Drive the line active, run `transfer`, then release it.
    ///
    /// The line goes back to its idle level even when `transfer` returns an error.
    pub fn select<R>(&mut self, transfer: impl FnOnce() -> R) -> R {
        let idle_high = self.pin.is_set_high();
        if idle_high {
            self.pin.set_low();
        } else {
            self.pin.set_high();
        }
        let result = transfer();
        if idle_high {
            self.pin.set_high();
        } else {
            self.pin.set_low();
        }
        result
    }
}

impl<'d, T: Instance, M: Mode> Drop for Spi<'d, T, M> {
    fn drop(&mut self) {
        // Drop the output enables. The vendor disable clears these three bits and
        // leaves the mode and divisor alone.
        T::regs().ctrl_mod().modify(|w| {
            w.set_sck_oe(false);
            w.set_mosi_oe(false);
            w.set_miso_oe(false);
        });
        self.sck.set_as_input(crate::gpio::Pull::Up);
        self.mosi.set_as_input(crate::gpio::Pull::Up);
        self.miso.set_as_input(crate::gpio::Pull::Up);
    }
}

/// Bytes the FIFO holds. The vendor header calls this `SPI_FIFO_SIZE`.
const FIFO_DEPTH: u8 = 8;

/// `TOTAL_CNT` is 12 bits wide.
const MAX_COUNT: usize = 0x0fff;

fn apply<T: Instance>(timing: &Timing, mode3: bool, bit_order: BitOrder) {
    let regs = T::regs();
    // Pulsing the clear empties a FIFO left by a previous owner before the mode
    // word is written, which is the vendor init order.
    regs.ctrl_mod().write(|w| w.set_all_clear(true));
    regs.ctrl_mod().write(|w| {
        w.set_mode_slave(false);
        w.set_mst_sck_mod(mode3);
        w.set_sck_oe(true);
        w.set_mosi_oe(true);
    });
    regs.ctrl_cfg().write(|w| {
        w.set_auto_if(true);
        w.set_bit_order(bit_order == BitOrder::LsbFirst);
        w.set_mst_dly_en(timing.input_delay);
    });
    regs.clock_div().write(|w| w.set_clock_div(timing.divisor));
}

/// Occupied FIFO entries, from the 4-bit count.
fn fifo_count<T: Instance>() -> Result<u8, Error> {
    let count = T::regs().fifo_count().read().fifo_count();
    if count > FIFO_DEPTH {
        Err(Error::Overrun)
    } else {
        Ok(count)
    }
}

/// One millisecond from now, in system-clock cycles.
fn deadline() -> u64 {
    cycles() + u64::from(crate::rcc::clocks().sysclk.0) / 1000
}

fn timed_out(deadline: u64) -> bool {
    cycles().wrapping_sub(deadline) < (1 << 63)
}

fn cycles() -> u64 {
    // The `riscv` crate is not a dependency, and the counter is only read here.
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
        fn regs() -> pac::spi::SpiFull;

        /// Set `PIN_ALTERNATE.PIN_SPI0`.
        ///
        /// # Safety
        ///
        /// The register is shared with every other peripheral, so the caller holds
        /// a critical section.
        unsafe fn set_remap(remapped: bool);
    }
}

/// An SPI controller with the CH585 FIFO layout.
#[allow(private_bounds)]
pub trait Instance: sealed::Instance + PeripheralType {
    /// Interrupt raised by this controller.
    ///
    /// `SPI1` shares its line with the PWM block, so this names the line rather
    /// than granting ownership of a handler.
    type Interrupt: interrupt::typelevel::Interrupt;

    /// Clock feeding the controller. Both controllers run at `Fsys`.
    fn frequency() -> Hertz {
        crate::rcc::clocks().sysclk
    }
}

impl sealed::Instance for peripherals::SPI0 {
    fn regs() -> pac::spi::SpiFull {
        pac::SPI0
    }

    unsafe fn set_remap(remapped: bool) {
        pac::SYS.pin_alternate().modify(|w| w.set_pin_spi0(remapped));
    }
}

impl Instance for peripherals::SPI0 {
    type Interrupt = interrupt::typelevel::SPI0;
}

// CH583 has the second controller too, but its family file lists no pins for
// it, so only CH585 exposes it.
#[cfg(ch585)]
impl sealed::Instance for peripherals::SPI1 {
    fn regs() -> pac::spi::SpiFull {
        // SPI1 is the same register block as SPI0 minus the DMA words, which
        // this driver never touches. Reading it through the full block keeps one
        // code path for both controllers.
        unsafe { pac::spi::SpiFull::from_ptr(pac::SPI1.as_ptr()) }
    }

    unsafe fn set_remap(_remapped: bool) {}
}

#[cfg(ch585)]
impl Instance for peripherals::SPI1 {
    type Interrupt = interrupt::typelevel::PWMX_SPI1;
}

mod pin_sealed {
    pub trait Sck {}
    pub trait Mosi {}
    pub trait Miso {}
}

/// SCK pin for `T` at remap `REMAP`.
pub trait SckPin<T: Instance, const REMAP: u8>: Pin + pin_sealed::Sck {}

/// MOSI pin for `T` at remap `REMAP`.
pub trait MosiPin<T: Instance, const REMAP: u8>: Pin + pin_sealed::Mosi {}

/// MISO pin for `T` at remap `REMAP`.
pub trait MisoPin<T: Instance, const REMAP: u8>: Pin + pin_sealed::Miso {}

// Unused on a die whose family file lists no SPI pins.
#[allow(unused_macros)]
macro_rules! impl_pins {
    ($inst:ident, $remap:literal, sck: $sck:ident, mosi: $mosi:ident, miso: $miso:ident) => {
        impl pin_sealed::Sck for peripherals::$sck {}
        impl SckPin<peripherals::$inst, $remap> for peripherals::$sck {}
        impl pin_sealed::Mosi for peripherals::$mosi {}
        impl MosiPin<peripherals::$inst, $remap> for peripherals::$mosi {}
        impl pin_sealed::Miso for peripherals::$miso {}
        impl MisoPin<peripherals::$inst, $remap> for peripherals::$miso {}
    };
}

// Datasheet table 7-8. `PIN_SPI0` clear is PA13/PA14/PA15, set is PB13/PB14/PB15.
// CH582 and CH583 share this register block but their family files carry no pin
// list, so the pins are named only where the metadata records them.
#[cfg(any(ch581, ch584, ch585))]
impl_pins!(SPI0, 0, sck: PA13, mosi: PA14, miso: PA15);
#[cfg(any(ch581, ch584, ch585))]
impl_pins!(SPI0, 1, sck: PB13, mosi: PB14, miso: PB15);
// SPI1 has no remap field. PA0 is SCK, PA1 is MOSI, PA2 is MISO. CH583 has the
// controller too, but no pins are recorded for it.
#[cfg(ch585)]
impl_pins!(SPI1, 0, sck: PA0, mosi: PA1, miso: PA2);

// --- embedded-hal ----------------------------------------------------------

impl<'d, T: Instance, M: Mode> embedded_hal::spi::ErrorType for Spi<'d, T, M> {
    type Error = Error;
}

impl<'d, T: Instance, M: Mode> embedded_hal::spi::SpiBus<u8> for Spi<'d, T, M> {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_read(words)
    }

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(words)
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        self.blocking_transfer(read, write)
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_transfer_in_place(words)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // Every transfer waits for `FREE` before it returns, so the bus is idle.
        Ok(())
    }
}
