//! SPI master.
//!
//! CH585 has two controllers, both clocked from `Fsys`. `SPI0` is a full
//! controller with a peripheral-local DMA engine; `SPI1` is master-only and has
//! no DMA block at all.
//!
//! Three paths share the block. [`Spi::new_blocking`] polls and moves one byte at
//! a time. [`Spi::new_async`] does the same under `CNT_END`/`FIFO_HF` interrupts,
//! which is the only way to do a full-duplex transfer because the FIFO can be
//! queued in one direction only. [`Spi::write_dma`] and [`Spi::read_dma`] use
//! `SPI0`'s local engine, which is also half-duplex: `FIFO_DIR` selects whether
//! the engine fills the transmit FIFO or drains the receive FIFO. There is no
//! channel controller and no request routing to configure.
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

#[cfg(feature = "embassy")]
use core::future::poll_fn;
use core::marker::PhantomData;
#[cfg(feature = "embassy")]
use core::sync::atomic::Ordering;
#[cfg(feature = "embassy")]
use core::task::Poll;

use embassy_hal_internal::PeripheralType;
#[cfg(feature = "embassy")]
use embassy_sync::waitqueue::AtomicWaker;
use embedded_hal::spi::{Phase, Polarity};

use crate::dma::{Descriptor, Dir};
use crate::gpio::{Flex, Level, Pin, Speed};
#[cfg(feature = "embassy")]
use crate::interrupt::typelevel::Interrupt;
#[cfg(feature = "embassy")]
use crate::mode::Async;
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
    /// A DMA buffer is empty, misaligned or outside the 128 KiB SRAM window.
    Buffer,
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
            Error::Timeout | Error::Buffer => ErrorKind::Other,
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
    /// The async constructor enabled this instance's interrupt line. `SPI1`
    /// shares `PWMX_SPI1`, so a blocking driver must not disable it on drop.
    irq_owned: bool,
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
            irq_owned: false,
            _phantom: PhantomData,
        })
    }
}

#[cfg(feature = "embassy")]
impl<'d, T: Instance> Spi<'d, T, Async> {
    /// Create an interrupt-driven master.
    ///
    /// `_irq` is the [`crate::bind_interrupts!`] binding for this instance.
    /// `SPI1` shares `PWMX_SPI1` with the PWM block, so only one handler can own
    /// that line.
    pub fn new_async<const REMAP: u8>(
        _peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
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
        idle_clock(&mut sck, mode3);
        mosi.set_low();
        sck.set_as_output(Speed::High);
        mosi.set_as_output(Speed::High);
        miso.set_as_input(crate::gpio::Pull::Up);
        critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });
        apply::<T>(&timing, mode3, config.bit_order);
        enable_interrupts::<T>();
        Ok(Self {
            sck,
            mosi,
            miso,
            irq_owned: true,
            _phantom: PhantomData,
        })
    }

    /// Write `bytes`, waiting for the FIFO each time it fills.
    ///
    /// The transfer is paced by the bus, so a busy executor can underrun the
    /// transmit FIFO at high clock rates. Use [`Spi::write_dma`] on `SPI0` for
    /// long or high-rate writes.
    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        write_async::<T>(bytes).await
    }

    /// Read `buffer.len()` bytes. The controller clocks `0xFF` out on MOSI.
    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        read_async::<T>(buffer).await
    }

    /// Write `write` and read into `read` at the same time.
    ///
    /// The CH585 FIFO can buffer only one direction, so a full-duplex transfer
    /// advances one byte per interrupt. This is the general path; unidirectional
    /// DMA is faster.
    pub async fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        transfer_async::<T>(read, write).await
    }

    /// Exchange `buffer` in place.
    pub async fn transfer_in_place(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        transfer_in_place_async::<T>(buffer).await
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
        self.transfer_all(bytes.len(), |i| bytes[i], |_, _| {})
    }

    /// Read `buffer.len()` bytes. The controller clocks `0xFF` out on MOSI.
    pub fn blocking_read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.transfer_all(buffer.len(), |_| 0xFF, |i, byte| buffer[i] = byte)
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
        self.transfer_all(
            length,
            |i| write.get(i).copied().unwrap_or(0xFF),
            |i, byte| {
                if let Some(slot) = read.get_mut(i) {
                    *slot = byte;
                }
            },
        )
    }

    fn transfer_all(
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

/// Interrupt handler shared by the async SPI paths.
///
/// `CNT_END` and `FIFO_OV` are edge events. `FIFO_HF` and `BYTE_END` are level
/// conditions tied to the FIFO state, so the handler masks them and the future
/// re-arms them after making progress; otherwise the handler would re-enter
/// until the executor gets a chance to run.
#[cfg(feature = "embassy")]
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

#[cfg(feature = "embassy")]
impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let r = T::regs();
        let state = T::state();
        let flags = r.int_flag().read();
        if flags.if_fifo_ov() {
            state.overflow.store(true, Ordering::Relaxed);
            r.int_flag().write(|w| w.set_if_fifo_ov(true));
        }
        if flags.if_cnt_end() {
            state.done.store(true, Ordering::Relaxed);
            r.int_flag().write(|w| w.set_if_cnt_end(true));
            // `IF_CNT_END` reads as set for as long as `TOTAL_CNT` is zero, so
            // an enabled source would re-enter here until a future runs. The
            // waits re-arm it after programming a new length.
            r.inter_en().modify(|w| w.set_ie_cnt_end(false));
        }
        if flags.if_dma_end() {
            state.dma_done.store(true, Ordering::Relaxed);
            r.int_flag().write(|w| w.set_if_dma_end(true));
            // One DMA completion per chunk; a second entry has nothing to add.
            r.inter_en().modify(|w| w.set_ie_dma_end(false));
        }
        if flags.if_fifo_hf() {
            r.int_flag().write(|w| w.set_if_fifo_hf(true));
            r.inter_en().modify(|w| w.set_ie_fifo_hf(false));
        }
        if flags.if_byte_end() {
            r.int_flag().write(|w| w.set_if_byte_end(true));
            r.inter_en().modify(|w| w.set_ie_byte_end(false));
        }
        state.waker.wake();
    }
}

#[cfg(feature = "embassy")]
fn enable_interrupts<T: Instance>() {
    let r = T::regs();
    r.int_flag().write(|w| {
        w.set_if_cnt_end(true);
        w.set_if_byte_end(true);
        w.set_if_fifo_hf(true);
        w.set_if_dma_end(true);
        w.set_if_fifo_ov(true);
    });
    // `IF_CNT_END` reads as set while `TOTAL_CNT` is zero, so enabling its
    // source here would storm the line the moment the vector is unmasked and
    // starve the caller of `new_async`. The waits arm it after a non-zero
    // length is programmed instead; the DMA path arms both flags in `arm`.
    r.inter_en().modify(|w| w.set_ie_fifo_ov(true));
    T::Interrupt::unpend();
    unsafe { T::Interrupt::enable() };
}

/// Wait until the FIFO can make progress, or the chunk finishes.
///
/// `tx` chooses the direction: a transmitter needs room, a receiver needs data.
#[cfg(feature = "embassy")]
async fn wait_progress<T: Instance>(tx: bool) -> Result<(), Error> {
    let r = T::regs();
    poll_fn(|cx| {
        let state = T::state();
        state.waker.register(cx.waker());
        if state.overflow.load(Ordering::Relaxed) {
            state.overflow.store(false, Ordering::Relaxed);
            return Poll::Ready(Err(Error::Overrun));
        }
        if state.done.load(Ordering::Relaxed) {
            return Poll::Ready(Ok(()));
        }
        let ready = || match fifo_count::<T>() {
            Ok(count) => {
                if tx {
                    count < FIFO_DEPTH
                } else {
                    count > 0
                }
            }
            Err(_) => true,
        };
        if ready() {
            r.inter_en().modify(|w| w.set_ie_fifo_hf(false));
            return Poll::Ready(fifo_count::<T>().map(|_| ()));
        }
        r.inter_en().modify(|w| w.set_ie_fifo_hf(true));
        // Close the race between the first check and arming the source.
        if ready() {
            r.inter_en().modify(|w| w.set_ie_fifo_hf(false));
            return Poll::Ready(fifo_count::<T>().map(|_| ()));
        }
        Poll::Pending
    })
    .await
}

/// Wait for the current chunk's `TOTAL_CNT` to reach zero.
#[cfg(feature = "embassy")]
async fn wait_done<T: Instance>() -> Result<(), Error> {
    let r = T::regs();
    let result = poll_fn(|cx| {
        let state = T::state();
        state.waker.register(cx.waker());
        if state.overflow.load(Ordering::Relaxed) {
            state.overflow.store(false, Ordering::Relaxed);
            return Poll::Ready(Err(Error::Overrun));
        }
        if state.done.load(Ordering::Relaxed) {
            return Poll::Ready(Ok(()));
        }
        // Arm the source only now that the caller has programmed a non-zero
        // `TOTAL_CNT`. At zero length the count-end flag reads as set, so a
        // source enabled at construction would re-enter the handler forever.
        r.inter_en().modify(|w| w.set_ie_cnt_end(true));
        // Close the race between the check above and arming the source.
        if r.int_flag().read().if_cnt_end() {
            r.int_flag().write(|w| w.set_if_cnt_end(true));
            return Poll::Ready(Ok(()));
        }
        Poll::Pending
    })
    .await;
    // Leave the source off so a completed transfer cannot keep re-asserting.
    r.inter_en().modify(|w| w.set_ie_cnt_end(false));
    result
}

/// Wait for one byte to be exchanged in the full-duplex path.
#[cfg(feature = "embassy")]
async fn wait_byte<T: Instance>() -> Result<(), Error> {
    let r = T::regs();
    poll_fn(|cx| {
        let state = T::state();
        state.waker.register(cx.waker());
        if state.overflow.load(Ordering::Relaxed) {
            state.overflow.store(false, Ordering::Relaxed);
            return Poll::Ready(Err(Error::Overrun));
        }
        if r.int_flag().read().if_byte_end() {
            r.int_flag().write(|w| w.set_if_byte_end(true));
            return Poll::Ready(Ok(()));
        }
        r.inter_en().modify(|w| w.set_ie_byte_end(true));
        if r.int_flag().read().if_byte_end() {
            r.int_flag().write(|w| w.set_if_byte_end(true));
            r.inter_en().modify(|w| w.set_ie_byte_end(false));
            return Poll::Ready(Ok(()));
        }
        Poll::Pending
    })
    .await
}

#[cfg(feature = "embassy")]
async fn write_async<T: Instance>(bytes: &[u8]) -> Result<(), Error> {
    let r = T::regs();
    r.ctrl_mod().modify(|w| w.set_fifo_dir(false));
    for chunk in bytes.chunks(MAX_COUNT) {
        let state = T::state();
        state.done.store(false, Ordering::Relaxed);
        state.overflow.store(false, Ordering::Relaxed);
        r.int_flag().write(|w| {
            w.set_if_cnt_end(true);
            w.set_if_fifo_ov(true);
            w.set_if_fifo_hf(true);
        });
        r.total_cnt().write(|w| w.set_total_cnt(chunk.len() as u16));
        let mut sent = 0;
        while sent < chunk.len() {
            while sent < chunk.len() && fifo_count::<T>()? < FIFO_DEPTH {
                r.fifo().write(|w| w.set_fifo(chunk[sent]));
                sent += 1;
            }
            if sent == chunk.len() {
                break;
            }
            wait_progress::<T>(true).await?;
        }
        wait_done::<T>().await?;
    }
    Ok(())
}

#[cfg(feature = "embassy")]
async fn read_async<T: Instance>(buffer: &mut [u8]) -> Result<(), Error> {
    let r = T::regs();
    r.ctrl_mod().modify(|w| w.set_fifo_dir(true));
    for chunk in buffer.chunks_mut(MAX_COUNT) {
        let state = T::state();
        state.done.store(false, Ordering::Relaxed);
        state.overflow.store(false, Ordering::Relaxed);
        r.int_flag().write(|w| {
            w.set_if_cnt_end(true);
            w.set_if_fifo_ov(true);
            w.set_if_fifo_hf(true);
        });
        r.total_cnt().write(|w| w.set_total_cnt(chunk.len() as u16));
        let mut got = 0;
        loop {
            while got < chunk.len() {
                if fifo_count::<T>()? == 0 {
                    break;
                }
                chunk[got] = r.fifo().read().fifo();
                got += 1;
            }
            if got == chunk.len() {
                break;
            }
            if state.overflow.load(Ordering::Relaxed) {
                state.overflow.store(false, Ordering::Relaxed);
                return Err(Error::Overrun);
            }
            if state.done.load(Ordering::Relaxed) {
                // The peripheral ended the count without delivering every byte.
                return Err(Error::Overrun);
            }
            wait_progress::<T>(false).await?;
        }
        // The last byte may still be shifting when the FIFO is already drained.
        wait_done::<T>().await?;
    }
    Ok(())
}

#[cfg(feature = "embassy")]
async fn transfer_one<T: Instance>(outgoing: u8) -> Result<u8, Error> {
    let r = T::regs();
    r.int_flag().write(|w| {
        w.set_if_byte_end(true);
        w.set_if_cnt_end(true);
    });
    r.total_cnt().write(|w| w.set_total_cnt(1));
    r.fifo().write(|w| w.set_fifo(outgoing));
    wait_byte::<T>().await?;
    // `FREE` is side-effect free; wait for the shifter, then take its byte.
    while !r.int_flag().read().free() {
        core::hint::spin_loop();
    }
    r.int_flag().write(|w| w.set_if_cnt_end(true));
    Ok(r.buffer().read().buffer())
}

#[cfg(feature = "embassy")]
async fn transfer_async<T: Instance>(read: &mut [u8], write: &[u8]) -> Result<(), Error> {
    let r = T::regs();
    r.ctrl_mod().modify(|w| w.set_fifo_dir(false));
    let length = read.len().max(write.len());
    for index in 0..length {
        let outgoing = write.get(index).copied().unwrap_or(0xFF);
        let incoming = transfer_one::<T>(outgoing).await?;
        if let Some(slot) = read.get_mut(index) {
            *slot = incoming;
        }
    }
    Ok(())
}

#[cfg(feature = "embassy")]
async fn transfer_in_place_async<T: Instance>(buffer: &mut [u8]) -> Result<(), Error> {
    let r = T::regs();
    r.ctrl_mod().modify(|w| w.set_fifo_dir(false));
    for slot in buffer.iter_mut() {
        let outgoing = *slot;
        *slot = transfer_one::<T>(outgoing).await?;
    }
    Ok(())
}

// --- SPI0 local DMA --------------------------------------------------------

mod dma_sealed {
    pub trait Instance {}
}

/// An SPI controller with a local DMA engine.
///
/// Only `SPI0` has one. `SPI1` is the `SPI_NODMA` register block: it has no
/// `DMA_BEG`, `DMA_END` or `DMA_ENABLE` bits at all, so this is a property of
/// the instance rather than of the driver.
#[allow(private_bounds)]
pub trait DmaInstance: Instance + dma_sealed::Instance {}

impl dma_sealed::Instance for peripherals::SPI0 {}
impl DmaInstance for peripherals::SPI0 {}

/// `TOTAL_CNT` is 12 bits; each internal chunk stays 32-bit aligned.
const DMA_CHUNK: usize = 0x0ffc;

/// A running SPI0 DMA transfer.
///
/// The DMA engine is half-duplex: `FIFO_DIR` selects whether memory is copied
/// into the transmit FIFO or out of the receive FIFO, and the other direction
/// cannot be serviced at the same time. That is why this type is separate from
/// the CPU-driven [`Spi`] paths rather than a `SpiBus` implementation.
///
/// Completion is `CNT_END`, matching the vendor driver. For a receive the
/// transfer also waits for `DMA_END` so the last SRAM store has happened before
/// the buffer is returned.
#[must_use = "dropping stops DMA; use blocking_wait, wait or stop"]
pub struct DmaTransfer<'a, 'd, T: DmaInstance, M: Mode> {
    /// Exclusive borrow of the controller for the transfer's lifetime.
    _spi: &'a mut Spi<'d, T, M>,
    buffer: Option<&'static mut [u8]>,
    descriptor: Option<Descriptor>,
    direction: Dir,
    offset: usize,
    chunk: usize,
    finished: bool,
}

impl<'d, T: DmaInstance, M: Mode> Spi<'d, T, M> {
    /// Copy `buffer` to MOSI with the DMA engine.
    pub fn write_dma(
        &mut self,
        buffer: &'static mut [u8],
    ) -> Result<DmaTransfer<'_, 'd, T, M>, (Error, &'static mut [u8])> {
        self.start_dma(buffer, Dir::MemoryToPeripheral)
    }

    /// Fill `buffer` from MISO with the DMA engine.
    pub fn read_dma(
        &mut self,
        buffer: &'static mut [u8],
    ) -> Result<DmaTransfer<'_, 'd, T, M>, (Error, &'static mut [u8])> {
        self.start_dma(buffer, Dir::PeripheralToMemory)
    }

    fn start_dma(
        &mut self,
        buffer: &'static mut [u8],
        direction: Dir,
    ) -> Result<DmaTransfer<'_, 'd, T, M>, (Error, &'static mut [u8])> {
        if !wait_idle::<T>() {
            return Err((Error::Timeout, buffer));
        }
        let descriptor = if buffer.is_empty() {
            None
        } else {
            match Descriptor::bytes(buffer.as_ptr() as usize, buffer.len()) {
                Ok(descriptor) => Some(descriptor),
                Err(_) => return Err((Error::Buffer, buffer)),
            }
        };
        let mut transfer = DmaTransfer {
            _spi: self,
            buffer: Some(buffer),
            descriptor,
            direction,
            offset: 0,
            chunk: 0,
            finished: descriptor.is_none(),
        };
        if let Err(error) = transfer.arm() {
            let buffer = transfer.buffer.take().unwrap();
            return Err((error, buffer));
        }
        Ok(transfer)
    }
}

impl<'a, 'd, T: DmaInstance, M: Mode> DmaTransfer<'a, 'd, T, M> {
    /// Bytes of `buffer` already handed to the controller.
    pub fn position(&self) -> usize {
        let Some(descriptor) = self.descriptor else {
            return 0;
        };
        let now = T::regs().dma_now().read().dma_now() as u32 & 0x0001_ffff;
        // `DMA_NOW` is the absolute SRAM offset, so this already counts every
        // completed chunk.
        let done = now.saturating_sub(descriptor.start) as usize;
        done.min(descriptor.len())
    }

    /// The whole buffer has been transferred.
    pub fn is_complete(&self) -> bool {
        self.finished
    }

    /// A receive FIFO overflow was reported by the controller.
    pub fn error(&self) -> Option<Error> {
        if T::regs().int_flag().read().if_fifo_ov() {
            Some(Error::Overrun)
        } else {
            None
        }
    }

    /// Wait for every chunk without blocking on an interrupt binding.
    pub fn blocking_wait(mut self) -> Result<&'static mut [u8], (Error, &'static mut [u8])> {
        loop {
            if let Some(error) = self.error() {
                self.quiesce();
                return Err((error, self.buffer.take().unwrap()));
            }
            if self.finished {
                self.quiesce();
                return Ok(self.buffer.take().unwrap());
            }
            if self.chunk_done() {
                if let Err(error) = self.settle() {
                    self.quiesce();
                    return Err((error, self.buffer.take().unwrap()));
                }
                if let Err(error) = self.advance() {
                    self.quiesce();
                    return Err((error, self.buffer.take().unwrap()));
                }
            } else {
                core::hint::spin_loop();
            }
        }
    }

    /// Stop the engine and hand the buffer back. The final chunk may be partial.
    pub fn stop(mut self) -> &'static mut [u8] {
        self.quiesce();
        self.buffer.take().unwrap()
    }

    fn arm(&mut self) -> Result<(), Error> {
        let Some(descriptor) = self.descriptor else {
            self.finished = true;
            return Ok(());
        };
        let remaining = descriptor.len() - self.offset;
        if remaining == 0 {
            self.finished = true;
            return Ok(());
        }
        let chunk = remaining.min(DMA_CHUNK);
        let start = descriptor.start + self.offset as u32;
        let end = start + chunk as u32;
        let r = T::regs();
        r.ctrl_cfg().modify(|w| w.set_dma_enable(false));
        r.ctrl_mod()
            .modify(|w| w.set_fifo_dir(self.direction == Dir::PeripheralToMemory));
        #[cfg(feature = "embassy")]
        {
            T::state().done.store(false, Ordering::Relaxed);
            T::state().dma_done.store(false, Ordering::Relaxed);
        }
        r.int_flag().write(|w| {
            w.set_if_cnt_end(true);
            w.set_if_dma_end(true);
            w.set_if_fifo_ov(true);
        });
        // Some dies encode the SRAM offset in a 16-bit register; their SRAM is
        // smaller than 64 KiB, so the truncation is lossless there.
        r.dma_beg().write(|w| w.set_dma_beg(start as _));
        r.dma_end().write(|w| w.set_dma_end(end as _));
        // The length goes in before the sources are enabled: `IF_CNT_END` reads
        // as set while `TOTAL_CNT` is zero, and a source enabled in that window
        // would keep re-entering the handler.
        r.total_cnt().write(|w| w.set_total_cnt(chunk as u16));
        crate::dma::fence();
        // Like the timer engine, the SPI's DMA_END flag is gated by its
        // interrupt enable even when the line is masked in the PFIC.
        r.inter_en().modify(|w| {
            w.set_ie_cnt_end(true);
            w.set_ie_dma_end(true);
        });
        r.ctrl_cfg().modify(|w| w.set_dma_enable(true));
        self.chunk = chunk;
        Ok(())
    }

    fn advance(&mut self) -> Result<(), Error> {
        self.offset += self.chunk;
        let done = self.descriptor.map_or(true, |d| self.offset >= d.len());
        if done {
            self.finished = true;
            self.quiesce();
            Ok(())
        } else {
            self.arm()
        }
    }

    fn chunk_done(&self) -> bool {
        #[cfg(feature = "embassy")]
        let count_end = T::state().done.load(Ordering::Relaxed);
        #[cfg(not(feature = "embassy"))]
        let count_end = false;
        let flags = T::regs().int_flag().read();
        if !(count_end || flags.if_cnt_end()) {
            return false;
        }
        if self.direction == Dir::PeripheralToMemory {
            // `CNT_END` says the last byte clocked in; `DMA_END` says the engine
            // stored it. Both are needed before the buffer is sound.
            #[cfg(feature = "embassy")]
            let dma_end = T::state().dma_done.load(Ordering::Relaxed);
            #[cfg(not(feature = "embassy"))]
            let dma_end = false;
            (dma_end || flags.if_dma_end()) && T::regs().fifo_count().read().fifo_count() == 0
        } else {
            true
        }
    }

    /// Wait, with a bound, for the receive engine to finish writing SRAM.
    fn settle(&mut self) -> Result<(), Error> {
        if self.direction != Dir::PeripheralToMemory {
            return Ok(());
        }
        let deadline = deadline();
        loop {
            #[cfg(feature = "embassy")]
            let dma_end = T::state().dma_done.load(Ordering::Relaxed);
            #[cfg(not(feature = "embassy"))]
            let dma_end = false;
            if (dma_end || T::regs().int_flag().read().if_dma_end()) && T::regs().fifo_count().read().fifo_count() == 0
            {
                return Ok(());
            }
            if timed_out(deadline) {
                return Err(Error::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    fn quiesce(&mut self) {
        let r = T::regs();
        r.ctrl_cfg().modify(|w| w.set_dma_enable(false));
        r.total_cnt().write(|w| w.set_total_cnt(0));
        r.int_flag().write(|w| {
            w.set_if_cnt_end(true);
            w.set_if_dma_end(true);
            w.set_if_fifo_ov(true);
        });
    }
}

impl<T: DmaInstance, M: Mode> Drop for DmaTransfer<'_, '_, T, M> {
    fn drop(&mut self) {
        self.quiesce();
    }
}

#[cfg(feature = "embassy")]
impl<T: DmaInstance> DmaTransfer<'_, '_, T, Async> {
    /// Wait for every chunk, using the async SPI interrupt handler.
    ///
    /// The driver must have been created with [`Spi::new_async`]; otherwise its
    /// interrupt line is not enabled and this future never wakes.
    pub async fn wait(
        mut self,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>>,
    ) -> Result<&'static mut [u8], (Error, &'static mut [u8])> {
        while !self.finished {
            if let Err(error) = wait_done::<T>().await {
                self.quiesce();
                return Err((error, self.buffer.take().unwrap()));
            }
            if let Err(error) = self.settle() {
                self.quiesce();
                return Err((error, self.buffer.take().unwrap()));
            }
            if let Err(error) = self.advance() {
                self.quiesce();
                return Err((error, self.buffer.take().unwrap()));
            }
        }
        self.quiesce();
        Ok(self.buffer.take().unwrap())
    }
}

/// Wait for the controller to be idle before starting DMA.
fn wait_idle<T: Instance>() -> bool {
    let deadline = deadline();
    loop {
        if T::regs().int_flag().read().free() && fifo_count::<T>() == Ok(0) {
            return true;
        }
        if timed_out(deadline) {
            return false;
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
        self.set_active(idle_high);
        let result = transfer();
        self.set_idle(idle_high);
        result
    }

    /// Drive the line active around an async transfer.
    ///
    /// Only the bus transaction is held; the line returns to idle when the
    /// future completes, is dropped or returns an error.
    #[cfg(feature = "embassy")]
    pub async fn select_async<R>(&mut self, transfer: impl core::future::Future<Output = R>) -> R {
        let idle_high = self.pin.is_set_high();
        self.set_active(idle_high);
        let result = transfer.await;
        self.set_idle(idle_high);
        result
    }

    fn set_active(&mut self, idle_high: bool) {
        if idle_high {
            self.pin.set_low();
        } else {
            self.pin.set_high();
        }
    }

    fn set_idle(&mut self, idle_high: bool) {
        if idle_high {
            self.pin.set_high();
        } else {
            self.pin.set_low();
        }
    }
}

impl<'d, T: Instance, M: Mode> Drop for Spi<'d, T, M> {
    fn drop(&mut self) {
        if self.irq_owned {
            #[cfg(feature = "embassy")]
            {
                T::regs().ctrl_cfg().modify(|w| w.set_dma_enable(false));
                T::regs().inter_en().write(|w| w.0 = 0);
                T::Interrupt::disable();
            }
        }
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

/// Wakers shared between the interrupt handler and the async transfers.
#[cfg(feature = "embassy")]
#[doc(hidden)]
pub struct State {
    waker: AtomicWaker,
    /// A chunk reached `CNT_END`; the ISR clears the source before waking.
    done: crate::atomic::AtomicBool,
    /// A DMA chunk reached `DMA_END`. Like the timer's flag, the SPI flag is
    /// gated by `IE_DMA_END`, so the ISR clears it and records it here.
    dma_done: crate::atomic::AtomicBool,
    /// `FIFO_OV` was observed; latched until the future reports it.
    overflow: crate::atomic::AtomicBool,
}

#[cfg(feature = "embassy")]
impl State {
    const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
            done: crate::atomic::AtomicBool::new(false),
            dma_done: crate::atomic::AtomicBool::new(false),
            overflow: crate::atomic::AtomicBool::new(false),
        }
    }
}

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

        #[cfg(feature = "embassy")]
        fn state() -> &'static super::State;
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

    #[cfg(feature = "embassy")]
    fn state() -> &'static State {
        static STATE: State = State::new();
        &STATE
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

    #[cfg(feature = "embassy")]
    fn state() -> &'static State {
        static STATE: State = State::new();
        &STATE
    }
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

#[cfg(feature = "embassy")]
impl<'d, T: Instance> embedded_hal_async::spi::SpiBus<u8> for Spi<'d, T, Async> {
    async fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.read(words).await
    }

    async fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.write(words).await
    }

    async fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        self.transfer(read, write).await
    }

    async fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.transfer_in_place(words).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        // Every async transfer waits for `CNT_END` before it returns.
        Ok(())
    }
}
