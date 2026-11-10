//! UART (asynchronous serial).
//!
//! CH585/CH584 have four 16C550-style UARTs. Unlike the CH32 USARTs, the baud
//! generator is a pair of integer dividers, not a fractional BRR:
//!
//! ```text
//! Fuart = Fsys * 2 / DIV
//! baud  = Fuart / 16 / DL
//! ```
//!
//! `DIV` is 7 bits (1..=127) and `DL` is 16 bits. This driver keeps `DIV = 1`
//! and rounds `DL = Fsys / (8 * baud)`, which is the closest integer ratio
//! available. That means the actual baud is quantised by the system clock; for
//! example the reset clock of ~5.333 MHz gives `DL = 6` at 115200, i.e. about
//! 111 kbaud. Read [`crate::rcc::clocks`] after calibration if the exact error
//! matters.
//!
//! Pin selection is the `SYS.PIN_ALTERNATE` bit for the instance, and the pin
//! traits are indexed by the remap value, so a default-remap pin cannot be
//! passed to a `REMAP = 1` constructor. The mapping itself lives in the
//! `ch5xx-data` family YAML (datasheet tables 1-1 and 7-8).
//!
//! [`Uart::new_blocking`] and [`Uart::new`] cover blocking and interrupt-driven
//! (embassy) use. The async halves share one interrupt per instance through
//! [`InterruptHandler`]; bind it with [`crate::bind_interrupts!`]. There is no
//! DMA in this HAL, so the async code moves bytes through the eight-byte FIFO
//! itself. The receiver is always enabled by the peripheral; `TXD_EN` gates the
//! transmit pin.
//!
//! [`UartRx::read`] and [`UartRx::blocking_read`] fill the whole buffer.
//! [`UartRx::read_some`] and [`UartRx::blocking_read_some`] instead wait only for
//! the first byte and return the available prefix. The short async read is
//! cancel-safe; exact reads and writes may consume a prefix if dropped. Standard
//! [`embedded_io`] and, with `embassy`, [`embedded_io_async`] traits are provided
//! for the full UART and its halves.
//!
//! The receive interrupt is level-based: `IIR` keeps reporting "receive data"
//! until `RBR` is read, so the handler masks `IER.RECV_RDY` and the read future
//! re-enables it after draining. Without that the ISR re-enters thousands of
//! times per byte and starves the executor. Wake-ups for the transmitter use
//! `TFC` because reading `IIR` clears the THR-empty indication.

use core::fmt;
#[cfg(feature = "embassy")]
use core::future::poll_fn;
use core::marker::PhantomData;
#[cfg(feature = "embassy")]
use core::sync::atomic::compiler_fence;
use core::sync::atomic::Ordering;
#[cfg(feature = "embassy")]
use core::task::Poll;

#[cfg(feature = "embassy")]
use embassy_sync::waitqueue::AtomicWaker;

use crate::gpio::{AnyPin, Pin, Pull, SealedPin, Speed};
#[cfg(feature = "embassy")]
use crate::internal::drop::OnDrop;
#[cfg(feature = "embassy")]
use crate::interrupt::typelevel::Interrupt;
#[cfg(not(feature = "embassy"))]
use crate::interrupt::typelevel::Interrupt as _;
#[cfg(feature = "embassy")]
use crate::mode::Async;
use crate::mode::{Blocking, Mode};
use crate::time::Hertz;
use crate::{interrupt, pac, peripherals, Peri};

/// UART data length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataBits {
    DataBits5 = 0,
    DataBits6 = 1,
    DataBits7 = 2,
    DataBits8 = 3,
}

/// UART parity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parity {
    None,
    Odd,
    Even,
    /// Parity bit is always 1 (mark).
    Mark,
    /// Parity bit is always 0 (space).
    Space,
}

/// UART stop bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopBits {
    Stop1 = 0,
    Stop2 = 1,
}

/// UART configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct Config {
    pub baudrate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

impl Config {
    /// 8N1 at `baudrate`.
    ///
    /// [`Config::default`] is 8N1 at 115200; use this when a device needs a
    /// different rate. The other fields can be assigned directly afterwards.
    pub const fn new(baudrate: u32) -> Self {
        Self {
            baudrate,
            data_bits: DataBits::DataBits8,
            parity: Parity::None,
            stop_bits: StopBits::Stop1,
        }
    }
}

impl Default for Config {
    /// 115200 8N1.
    fn default() -> Self {
        Self {
            baudrate: 115200,
            data_bits: DataBits::DataBits8,
            parity: Parity::None,
            stop_bits: StopBits::Stop1,
        }
    }
}

/// UART error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// Receiver overrun. The flag is cleared when `LSR` is read.
    Overrun,
    /// Parity error on the byte just read.
    Parity,
    /// Missing stop bit.
    Framing,
    /// BREAK condition detected.
    Break,
}

/// Configuration error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfigError {
    /// The requested baud cannot be represented with `DL <= 65535`.
    BaudrateTooLow,
    /// `DL` would round to zero.
    BaudrateTooHigh,
    /// The nearest divider is more than two percent off the requested rate.
    Baudrate,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Overrun => "receiver overrun",
            Error::Parity => "parity error",
            Error::Framing => "framing error",
            Error::Break => "break condition",
        })
    }
}

impl core::error::Error for Error {}

impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        embedded_io::ErrorKind::Other
    }
}

/// One UART instance.
///
/// Implemented for `UART0`..`UART3`.
#[allow(private_bounds)]
pub trait Instance: crate::PeripheralType + SealedInstance + 'static + Send {
    /// Interrupt raised by this instance.
    type Interrupt: interrupt::typelevel::Interrupt;

    /// Clock feeding the baud generator.
    #[inline]
    fn frequency() -> Hertz {
        crate::rcc::clocks().sysclk
    }
}

/// Wakers shared between the interrupt handler and the async futures.
pub(crate) struct State {
    /// Number of live TX/RX owners, so the last half can disable the shared IRQ.
    owners: crate::atomic::AtomicU8,
    #[cfg(feature = "embassy")]
    rx_waker: AtomicWaker,
    #[cfg(feature = "embassy")]
    tx_waker: AtomicWaker,
    /// First line-status error seen by the interrupt handler or deferred by a
    /// short read: 0 none, 1 overrun, 2 parity, 3 framing, 4 break.
    rx_error: crate::atomic::AtomicU8,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self {
            owners: crate::atomic::AtomicU8::new(0),
            #[cfg(feature = "embassy")]
            rx_waker: AtomicWaker::new(),
            #[cfg(feature = "embassy")]
            tx_waker: AtomicWaker::new(),
            rx_error: crate::atomic::AtomicU8::new(0),
        }
    }
}

pub(crate) trait SealedInstance {
    fn regs() -> pac::uart::UartFull;

    /// Whether `MCR.LOOP` exists. `UART_NOMODEM` (UART1-3 on CH592, CH583,
    /// CH595, CH573) drops the modem block, including the loopback bit.
    const INTERNAL_LOOPBACK: bool;

    /// Set the `PIN_ALTERNATE` bit that selects the remapped pins.
    ///
    /// # Safety
    ///
    /// The remap register is shared with every other peripheral, so the caller
    /// must hold a critical section.
    unsafe fn set_remap(remapped: bool);

    /// Per-instance state and interrupt wakers.
    fn state() -> &'static State;
}

/// Registers shared by the full and the no-modem block sit at the same
/// offsets, so the driver always views the instance as `UartFull`. The
/// no-modem block only differs in two places: its THR write field is named
/// `RBR` (same bits) and it has no `MCR.LOOP`.
macro_rules! impl_instance {
    ($inst:ident, $setter:ident, $loopback:expr) => {
        impl SealedInstance for peripherals::$inst {
            #[inline]
            fn regs() -> pac::uart::UartFull {
                // Safety: the two blocks use the same register addresses for
                // the registers this driver touches.
                unsafe { pac::uart::UartFull::from_ptr(pac::$inst.as_ptr()) }
            }

            const INTERNAL_LOOPBACK: bool = $loopback;

            #[inline]
            unsafe fn set_remap(remapped: bool) {
                pac::SYS.pin_alternate().modify(|w| w.$setter(remapped));
            }

            #[inline]
            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }

        impl Instance for peripherals::$inst {
            type Interrupt = interrupt::typelevel::$inst;
        }
    };
}

/// Every UART is the full block unless the family selects `UART_NOMODEM` for
/// that instance, which CH592, CH583, CH595 and CH573 do for UART1-3.
macro_rules! impl_uart {
    ($inst:ident, $setter:ident, $cfg:ident) => {
        #[cfg(not($cfg))]
        impl_instance!($inst, $setter, true);
        #[cfg($cfg)]
        impl_instance!($inst, $setter, false);
    };
}

impl_uart!(UART0, set_pin_uart0, uart0_nomodem);
impl_uart!(UART1, set_pin_uart1, uart1_nomodem);
impl_uart!(UART2, set_pin_uart2, uart2_nomodem);
impl_uart!(UART3, set_pin_uart3, uart3_nomodem);

/// Interrupt handler shared by all async UART instances.
///
/// `IIR` identifies the source: `0b001` is "THR empty", `0b010`/`0b110` are
/// receive-data and character-timeout, and `0b011` is receive-line-status.
/// The PAC's `IIR_INT_MASK` field is already the identification bits `[3:1]`
/// (datasheet table 9-5); the no-interrupt flag in bit 0 is separate.
/// `LSR` must be read on a line-status interrupt to deassert it (the four error
/// bits are read-clear), so the error kind is saved in `State` for the read
/// future to report. Transmit wake-ups use `TFC` instead of the IIR THR-empty
/// bit because a pending line status has priority and reading `IIR` clears the
/// THR-empty indication.
#[cfg(feature = "embassy")]
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

#[cfg(feature = "embassy")]
impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let r = T::regs();
        let iir = r.iir().read();
        if iir.iir_no_int() {
            return;
        }

        let state = T::state();
        match iir.iir_int_mask() {
            // Receive-line-status: LSR is read-clear, so the handler must
            // consume it or the interrupt line stays asserted forever. Save
            // the error kind for the read future instead of touching LSR from
            // the future later.
            0b011 => {
                let lsr = r.lsr().read();
                if let Some(error) = lsr_error(lsr) {
                    save_rx_error::<T>(error);
                }
                state.rx_waker.wake();
            }
            // Receive data / character timeout. Only reading `RBR` clears the
            // condition, and only the read future does that, so a level
            // interrupt would re-enter this handler until the executor gets a
            // chance to run - measured as thousands of entries per byte. Mask
            // the source and let the future re-enable it after draining.
            0b010 | 0b110 => {
                r.ier().modify(|w| w.set_ier_recv_rdy(false));
                state.rx_waker.wake();
            }
            _ => {}
        }

        // Do not wait for the IIR THR-empty indication: a pending line-status
        // interrupt takes priority over it, and reading IIR clears the
        // THR-empty condition until the next write. `TFC` is side-effect free,
        // so wake the writer whenever the FIFO has drained.
        if r.tfc().read().tfc() == 0 {
            state.tx_waker.wake();
        }
    }
}

mod pin_sealed {
    pub trait Tx {}
    pub trait Rx {}
}

/// Pin that can carry UART transmit for `T` at remap `REMAP`.
pub trait TxPin<T: Instance, const REMAP: u8>: Pin + pin_sealed::Tx {}

/// Pin that can carry UART receive for `T` at remap `REMAP`.
pub trait RxPin<T: Instance, const REMAP: u8>: Pin + pin_sealed::Rx {}

macro_rules! impl_pins {
    ($inst:ident, $remap:literal, rx: [$($rx:ident),* $(,)?], tx: [$($tx:ident),* $(,)?]) => {
        $(
            impl pin_sealed::Rx for peripherals::$rx {}
            impl RxPin<peripherals::$inst, $remap> for peripherals::$rx {}
        )*
        $(
            impl pin_sealed::Tx for peripherals::$tx {}
            impl TxPin<peripherals::$inst, $remap> for peripherals::$tx {}
        )*
    };
}

// Datasheet table 7-8, default column.
impl_pins!(UART0, 0, rx: [PB4], tx: [PB7]);
impl_pins!(UART1, 0, rx: [PA8], tx: [PA9]);
impl_pins!(UART2, 0, rx: [PA6], tx: [PA7]);
impl_pins!(UART3, 0, rx: [PA4], tx: [PA5]);
// Datasheet table 7-8, remapped column.
impl_pins!(UART0, 1, rx: [PA15], tx: [PA14]);
impl_pins!(UART1, 1, rx: [PB12], tx: [PB13]);
impl_pins!(UART2, 1, rx: [PB22], tx: [PB23]);
impl_pins!(UART3, 1, rx: [PB20], tx: [PB21]);

/// Transmit half of a UART.
pub struct UartTx<'d, T: Instance, M: Mode> {
    _phantom: PhantomData<(T, M)>,
    tx: Peri<'d, AnyPin>,
}

impl<'d, T: Instance, M: Mode> UartTx<'d, T, M> {
    /// Perform a blocking write.
    pub fn blocking_write(&mut self, buffer: &[u8]) -> Result<(), Error> {
        let r = T::regs();
        for &byte in buffer {
            // TFC counts bytes waiting in the 8-byte TX FIFO.
            while r.tfc().read().tfc() >= 8 {}
            r.thr().write(|w| w.set_thr(byte));
        }
        Ok(())
    }

    /// Wait until the FIFO and the shift register are both empty.
    pub fn blocking_flush(&mut self) -> Result<(), Error> {
        while !read_lsr_for_tx::<T>().lsr_tx_all_emp() {}
        Ok(())
    }

    fn new_inner(tx: Peri<'d, AnyPin>) -> Self {
        T::state().owners.fetch_add(1, Ordering::Relaxed);
        Self {
            _phantom: PhantomData,
            tx,
        }
    }
}

impl<T: Instance, M: Mode> Drop for UartTx<'_, T, M> {
    fn drop(&mut self) {
        let r = T::regs();
        r.ier().modify(|w| {
            w.set_ier_thr_empty(false);
            w.set_ier_txd_en(false);
        });
        // The receiver is always enabled, so RXD needs no separate disable.
        if T::state().owners.fetch_sub(1, Ordering::Relaxed) == 1 {
            T::Interrupt::disable();
            r.mcr().modify(|w| w.set_mcr_int_oe(false));
        }
        // Leave the pin as a pulled-up input, not a driven output. The global
        // mux is shared with other UART users and is intentionally left alone.
        self.tx.set_direction(false);
        self.tx.set_pull(Pull::Up);
    }
}

impl<'d, T: Instance> UartTx<'d, T, Blocking> {
    /// Create a transmit-only UART.
    pub fn new_blocking<const REMAP: u8>(
        _peri: Peri<'d, T>,
        tx: Peri<'d, impl TxPin<T, REMAP>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        configure::<T, REMAP>(&config, true)?;
        let tx = tx.into();
        configure_tx_pin(&tx);
        Ok(Self::new_inner(tx))
    }
}

#[cfg(feature = "embassy")]
impl<'d, T: Instance> UartTx<'d, T, Async> {
    /// Create a transmit-only UART driven by interrupts.
    ///
    /// `_irq` is the [`crate::bind_interrupts!`] binding for this instance.
    pub fn new<const REMAP: u8>(
        _peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        tx: Peri<'d, impl TxPin<T, REMAP>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        configure::<T, REMAP>(&config, true)?;
        let tx = tx.into();
        configure_tx_pin(&tx);
        enable_interrupt_output::<T>();
        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };
        Ok(Self::new_inner(tx))
    }

    /// Queue `buffer` into the transmit FIFO, waiting for room when it is full.
    ///
    /// Returns once the whole buffer has been handed to the peripheral; use
    /// [`UartTx::flush`] if the last byte must be on the wire. Dropping the future
    /// before completion may leave a prefix queued; it is not cancel-safe.
    pub async fn write(&mut self, buffer: &[u8]) -> Result<(), Error> {
        write_async::<T>(buffer).await
    }

    /// Wait until the FIFO and the shift register are both empty.
    ///
    /// CH585 UARTs have no transmission-complete interrupt, so this yields
    /// cooperatively while polling `LSR.TX_ALL_EMP`.
    pub async fn flush(&mut self) -> Result<(), Error> {
        flush_async::<T>().await
    }
}

impl<'d, T: Instance, M: Mode> fmt::Write for UartTx<'d, T, M> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.blocking_write(s.as_bytes()).map_err(|_| fmt::Error)
    }
}

/// Receive half of a UART.
pub struct UartRx<'d, T: Instance, M: Mode> {
    _phantom: PhantomData<(T, M)>,
    _rx: Peri<'d, AnyPin>,
}

impl<'d, T: Instance, M: Mode> UartRx<'d, T, M> {
    /// Perform a blocking read, filling `buffer` completely.
    ///
    /// If a line-status error follows a partial read, the bytes already read
    /// remain in `buffer` but the call returns the error. Call again to resume.
    pub fn blocking_read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        read_blocking::<T>(buffer, true).map(|_| ())
    }

    /// Read whatever is available, blocking only for the first byte.
    ///
    /// If an error follows one or more valid bytes, this returns those bytes and
    /// reports the error on the next call, so the valid prefix is not lost.
    pub fn blocking_read_some(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        read_blocking::<T>(buffer, false)
    }

    fn new_inner(rx: Peri<'d, AnyPin>) -> Self {
        T::state().owners.fetch_add(1, Ordering::Relaxed);
        Self {
            _phantom: PhantomData,
            _rx: rx,
        }
    }
}

impl<T: Instance, M: Mode> Drop for UartRx<'_, T, M> {
    fn drop(&mut self) {
        let r = T::regs();
        r.ier().modify(|w| {
            w.set_ier_recv_rdy(false);
            w.set_ier_line_stat(false);
        });
        // Both halves share one PFIC interrupt. Dropping either one masks the
        // sources it owns; dropping the last one disables the IRQ itself.
        if T::state().owners.fetch_sub(1, Ordering::Relaxed) == 1 {
            T::Interrupt::disable();
            r.mcr().modify(|w| w.set_mcr_int_oe(false));
        }
    }
}

impl<'d, T: Instance> UartRx<'d, T, Blocking> {
    /// Create a receive-only UART.
    pub fn new_blocking<const REMAP: u8>(
        _peri: Peri<'d, T>,
        rx: Peri<'d, impl RxPin<T, REMAP>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let rx = rx.into();
        // The pin comes first: the receiver is always enabled, so it starts
        // sampling as soon as the peripheral is reset.
        configure_rx_pin(&rx);
        configure::<T, REMAP>(&config, false)?;
        Ok(Self::new_inner(rx))
    }
}

#[cfg(feature = "embassy")]
impl<'d, T: Instance> UartRx<'d, T, Async> {
    /// Create a receive-only UART driven by interrupts.
    ///
    /// `_irq` is the [`crate::bind_interrupts!`] binding for this instance.
    pub fn new<const REMAP: u8>(
        _peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        rx: Peri<'d, impl RxPin<T, REMAP>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let rx = rx.into();
        // The pin comes first: the receiver is always enabled, so it starts
        // sampling as soon as the peripheral is reset.
        configure_rx_pin(&rx);
        configure::<T, REMAP>(&config, false)?;
        enable_interrupt_output::<T>();
        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };
        Ok(Self::new_inner(rx))
    }

    /// Read until `buffer` is full.
    ///
    /// Returns immediately with an error if a line-status error is pending; the
    /// `LSR` read that reports it also clears the error bits. Valid bytes already
    /// read into `buffer` are retained when a later byte reports an error.
    /// Dropping the future leaves bytes already read in `buffer`; those bytes are
    /// consumed and the rest of `buffer` is still unwritten.
    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        read_async::<T>(buffer, true).await.map(|_| ())
    }

    /// Read whatever is available, waiting only for the first byte.
    ///
    /// Unlike [`UartRx::read`], this returns after at least one byte instead of
    /// waiting to fill the whole buffer. It is cancel-safe: no bytes are consumed
    /// if the future is dropped before returning.
    pub async fn read_some(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        read_async::<T>(buffer, false).await
    }
}

/// Bidirectional UART.
pub struct Uart<'d, T: Instance, M: Mode> {
    tx: UartTx<'d, T, M>,
    rx: UartRx<'d, T, M>,
}

impl<'d, T: Instance, M: Mode> Uart<'d, T, M> {
    /// Perform a blocking write.
    pub fn blocking_write(&mut self, buffer: &[u8]) -> Result<(), Error> {
        self.tx.blocking_write(buffer)
    }

    /// Wait until the transmitter is idle.
    pub fn blocking_flush(&mut self) -> Result<(), Error> {
        self.tx.blocking_flush()
    }

    /// Perform a blocking read, filling `buffer` completely.
    pub fn blocking_read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.rx.blocking_read(buffer)
    }

    /// Read whatever is available, blocking only for the first byte.
    pub fn blocking_read_some(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        self.rx.blocking_read_some(buffer)
    }

    /// Split into independent transmitter and receiver halves.
    pub fn split(self) -> (UartTx<'d, T, M>, UartRx<'d, T, M>) {
        (self.tx, self.rx)
    }

    /// Enable or disable the 16C550 internal loopback (`MCR.LOOP`).
    ///
    /// TXD is connected to RXD inside the peripheral and all external UART
    /// outputs become inactive, so this is a self-test hook, not a normal
    /// serial mode. Enabling it starts from an empty RX FIFO, so bytes that
    /// arrived before the switch are not mistaken for looped-back data.
    ///
    /// Measured on CH585: the UART interrupt output is not asserted while
    /// `MCR_LOOP` is set, so loopback has to be driven by polling. Use it with
    /// the blocking constructors, not the async ones.
    pub fn set_internal_loopback(&mut self, enable: bool) {
        let r = T::regs();
        assert!(
            !enable || T::INTERNAL_LOOPBACK,
            "this UART instance has no internal loopback"
        );
        if T::INTERNAL_LOOPBACK {
            r.mcr().modify(|w| w.set_mcr_loop(enable));
        }
        if enable {
            // Preserve FIFO mode: writing FCR with only the clear bit would
            // also clear `FCR_FIFO_EN`, leaving a one-byte receiver.
            r.fcr().write(|w| {
                w.set_fcr_fifo_en(true);
                w.set_fcr_rx_fifo_clr(true);
                w.set_fcr_fifo_trig(0);
            });
            let _ = r.lsr().read();
        }
    }
}

impl<'d, T: Instance> Uart<'d, T, Blocking> {
    /// Create a bidirectional UART.
    pub fn new_blocking<const REMAP: u8>(
        _peri: Peri<'d, T>,
        rx: Peri<'d, impl RxPin<T, REMAP>>,
        tx: Peri<'d, impl TxPin<T, REMAP>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let rx = rx.into();
        let tx = tx.into();
        // The RX pin comes first: the receiver is always enabled, so it starts
        // sampling as soon as the peripheral is reset.
        configure_rx_pin(&rx);
        configure_tx_pin(&tx);
        configure::<T, REMAP>(&config, true)?;
        Ok(Self {
            tx: UartTx::new_inner(tx),
            rx: UartRx::new_inner(rx),
        })
    }
}

#[cfg(feature = "embassy")]
impl<'d, T: Instance> Uart<'d, T, Async> {
    /// Create a bidirectional UART driven by interrupts.
    ///
    /// `_irq` is the [`crate::bind_interrupts!`] binding for this instance.
    pub fn new<const REMAP: u8>(
        _peri: Peri<'d, T>,
        rx: Peri<'d, impl RxPin<T, REMAP>>,
        tx: Peri<'d, impl TxPin<T, REMAP>>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let rx = rx.into();
        let tx = tx.into();
        // The RX pin comes first: the receiver is always enabled, so it starts
        // sampling as soon as the peripheral is reset.
        configure_rx_pin(&rx);
        configure_tx_pin(&tx);
        configure::<T, REMAP>(&config, true)?;
        enable_interrupt_output::<T>();
        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };
        Ok(Self {
            tx: UartTx::new_inner(tx),
            rx: UartRx::new_inner(rx),
        })
    }

    /// Queue `buffer` into the transmit FIFO, waiting for room when it is full.
    pub async fn write(&mut self, buffer: &[u8]) -> Result<(), Error> {
        self.tx.write(buffer).await
    }

    /// Wait until the transmitter is idle.
    pub async fn flush(&mut self) -> Result<(), Error> {
        self.tx.flush().await
    }

    /// Read until `buffer` is full.
    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.rx.read(buffer).await
    }

    /// Read whatever is available, waiting only for the first byte.
    pub async fn read_some(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        self.rx.read_some(buffer).await
    }
}

impl<'d, T: Instance, M: Mode> fmt::Write for Uart<'d, T, M> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.blocking_write(s.as_bytes()).map_err(|_| fmt::Error)
    }
}

impl<T: Instance, M: Mode> embedded_io::ErrorType for Uart<'_, T, M> {
    type Error = Error;
}

impl<T: Instance, M: Mode> embedded_io::ErrorType for UartTx<'_, T, M> {
    type Error = Error;
}

impl<T: Instance, M: Mode> embedded_io::ErrorType for UartRx<'_, T, M> {
    type Error = Error;
}

impl<T: Instance, M: Mode> embedded_io::Read for Uart<'_, T, M> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        self.blocking_read_some(buffer)
    }
}

impl<T: Instance, M: Mode> embedded_io::Read for UartRx<'_, T, M> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        self.blocking_read_some(buffer)
    }
}

impl<T: Instance, M: Mode> embedded_io::Write for Uart<'_, T, M> {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        self.blocking_write(buffer)?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush()
    }
}

impl<T: Instance, M: Mode> embedded_io::Write for UartTx<'_, T, M> {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        self.blocking_write(buffer)?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush()
    }
}

impl<T: Instance, M: Mode> embedded_io::ReadReady for Uart<'_, T, M> {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        // RFC has no read-clear side effects, unlike LSR.
        Ok(T::regs().rfc().read().rfc() != 0 || T::state().rx_error.load(Ordering::Relaxed) != 0)
    }
}

impl<T: Instance, M: Mode> embedded_io::ReadReady for UartRx<'_, T, M> {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(T::regs().rfc().read().rfc() != 0 || T::state().rx_error.load(Ordering::Relaxed) != 0)
    }
}

impl<T: Instance, M: Mode> embedded_io::WriteReady for Uart<'_, T, M> {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(T::regs().tfc().read().tfc() < 8)
    }
}

impl<T: Instance, M: Mode> embedded_io::WriteReady for UartTx<'_, T, M> {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(T::regs().tfc().read().tfc() < 8)
    }
}

#[cfg(feature = "embassy")]
impl<T: Instance> embedded_io_async::Read for Uart<'_, T, Async> {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        self.read_some(buffer).await
    }
}

#[cfg(feature = "embassy")]
impl<T: Instance> embedded_io_async::Read for UartRx<'_, T, Async> {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        self.read_some(buffer).await
    }
}

#[cfg(feature = "embassy")]
impl<T: Instance> embedded_io_async::Write for Uart<'_, T, Async> {
    async fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        Uart::write(self, buffer).await?;
        Ok(buffer.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Uart::flush(self).await
    }
}

#[cfg(feature = "embassy")]
impl<T: Instance> embedded_io_async::Write for UartTx<'_, T, Async> {
    async fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        UartTx::write(self, buffer).await?;
        Ok(buffer.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        UartTx::flush(self).await
    }
}

/// Route the UART interrupt to the core.
///
/// `MCR_INT_OE` (the 16C550 `OUT2` bit) gates the interrupt line itself; without
/// it `IER` bits and a pending `IIR` condition never reach the PFIC. WCH's
/// `UARTx_INTCfg` sets it whenever it enables an interrupt.
#[cfg(feature = "embassy")]
fn enable_interrupt_output<T: Instance>() {
    T::regs().mcr().modify(|w| w.set_mcr_int_oe(true));
}

/// Queue bytes into the eight-byte TX FIFO and wait for room when it fills.
#[cfg(feature = "embassy")]
async fn write_async<T: Instance>(buffer: &[u8]) -> Result<(), Error> {
    if buffer.is_empty() {
        return Ok(());
    }

    let r = T::regs();
    let state = T::state();
    let mut written = 0usize;

    // A dropped write future must not leave the THR-empty interrupt running.
    let on_drop = OnDrop::new(|| r.ier().modify(|w| w.set_ier_thr_empty(false)));

    poll_fn(|cx| {
        state.tx_waker.register(cx.waker());

        while written < buffer.len() && r.tfc().read().tfc() < 8 {
            r.thr().write(|w| w.set_thr(buffer[written]));
            written += 1;
        }

        if written == buffer.len() {
            r.ier().modify(|w| w.set_ier_thr_empty(false));
            return Poll::Ready(());
        }

        // FIFO full: ask for an interrupt when it drains, then check once more
        // so an interrupt that fired between the check and the enable is not
        // lost.
        r.ier().modify(|w| w.set_ier_thr_empty(true));
        compiler_fence(Ordering::SeqCst);

        while written < buffer.len() && r.tfc().read().tfc() < 8 {
            r.thr().write(|w| w.set_thr(buffer[written]));
            written += 1;
        }

        if written == buffer.len() {
            r.ier().modify(|w| w.set_ier_thr_empty(false));
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;

    on_drop.defuse();
    Ok(())
}

/// Wait for `LSR.TX_ALL_EMP`.
///
/// There is no transmission-complete interrupt to wait on, so the future
/// re-schedules itself and polls. RX error bits read as a side effect are saved
/// for the receiver instead of being silently discarded.
#[cfg(feature = "embassy")]
async fn flush_async<T: Instance>() -> Result<(), Error> {
    poll_fn(|cx| {
        if read_lsr_for_tx::<T>().lsr_tx_all_emp() {
            Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
    Ok(())
}

/// Fill `buffer` from the RX FIFO, waking on the receive interrupts.
///
/// Exact reads may consume a prefix before returning `Pending`; short reads only
/// consume bytes in a poll that returns `Ready`, making them cancel-safe.
#[cfg(feature = "embassy")]
async fn read_async<T: Instance>(buffer: &mut [u8], wait_for_full: bool) -> Result<usize, Error> {
    if buffer.is_empty() {
        return Ok(0);
    }

    let r = T::regs();
    let state = T::state();
    let mut filled = 0usize;

    // A dropped read future must not leave the receive interrupts running.
    let on_drop = OnDrop::new(|| {
        r.ier().modify(|w| {
            w.set_ier_recv_rdy(false);
            w.set_ier_line_stat(false);
        })
    });

    let result = poll_fn(|cx| {
        state.rx_waker.register(cx.waker());

        // Enable before checking, so bytes that arrive while this future is
        // returning `Pending` raise the interrupt that wakes it again.
        r.ier().modify(|w| {
            w.set_ier_recv_rdy(true);
            w.set_ier_line_stat(true);
        });
        compiler_fence(Ordering::SeqCst);

        loop {
            if filled == buffer.len() {
                return Poll::Ready(Ok(filled));
            }

            // The interrupt handler consumes line-status errors (LSR is
            // read-clear) and records the first one for this future.
            if let Some(error) = take_rx_error::<T>() {
                if filled > 0 && !wait_for_full {
                    restore_rx_error::<T>(error);
                    return Poll::Ready(Ok(filled));
                }
                return Poll::Ready(Err(error));
            }

            let lsr = r.lsr().read();
            if let Some(error) = lsr_error(lsr) {
                if filled > 0 && !wait_for_full {
                    save_rx_error::<T>(error);
                    return Poll::Ready(Ok(filled));
                }
                return Poll::Ready(Err(error));
            }
            if lsr.lsr_data_rdy() {
                buffer[filled] = r.rbr().read().rbr();
                filled += 1;
                continue;
            }
            if filled > 0 && !wait_for_full {
                return Poll::Ready(Ok(filled));
            }

            return Poll::Pending;
        }
    })
    .await;

    r.ier().modify(|w| {
        w.set_ier_recv_rdy(false);
        w.set_ier_line_stat(false);
    });
    on_drop.defuse();

    result
}

/// Read bytes from the RX FIFO. Exact reads wait for a full buffer; short reads
/// return once at least one byte is available and the FIFO has been drained.
fn read_blocking<T: Instance>(buffer: &mut [u8], wait_for_full: bool) -> Result<usize, Error> {
    if buffer.is_empty() {
        return Ok(0);
    }

    let r = T::regs();
    let mut filled = 0usize;
    loop {
        if filled == buffer.len() {
            return Ok(filled);
        }

        if let Some(error) = take_rx_error::<T>() {
            if filled > 0 && !wait_for_full {
                restore_rx_error::<T>(error);
                return Ok(filled);
            }
            return Err(error);
        }

        let lsr = r.lsr().read();
        if let Some(error) = lsr_error(lsr) {
            if filled > 0 && !wait_for_full {
                save_rx_error::<T>(error);
                return Ok(filled);
            }
            return Err(error);
        }
        if lsr.lsr_data_rdy() {
            buffer[filled] = r.rbr().read().rbr();
            filled += 1;
        } else if filled > 0 && !wait_for_full {
            return Ok(filled);
        }
    }
}

/// Decode the read-clear line-status error bits.
fn lsr_error(lsr: pac::uart::regs::Lsr) -> Option<Error> {
    if lsr.lsr_over_err() {
        Some(Error::Overrun)
    } else if lsr.lsr_par_err() {
        Some(Error::Parity)
    } else if lsr.lsr_frame_err() {
        Some(Error::Framing)
    } else if lsr.lsr_break_err() {
        Some(Error::Break)
    } else {
        None
    }
}

fn error_code(error: Error) -> u8 {
    match error {
        Error::Overrun => 1,
        Error::Parity => 2,
        Error::Framing => 3,
        Error::Break => 4,
    }
}

/// Preserve the first pending error instead of overwriting it with a later one.
fn save_rx_error<T: Instance>(error: Error) {
    let _ = T::state()
        .rx_error
        .compare_exchange(0, error_code(error), Ordering::Relaxed, Ordering::Relaxed);
}

fn take_rx_error<T: Instance>() -> Option<Error> {
    match T::state().rx_error.swap(0, Ordering::Relaxed) {
        1 => Some(Error::Overrun),
        2 => Some(Error::Parity),
        3 => Some(Error::Framing),
        4 => Some(Error::Break),
        _ => None,
    }
}

/// Put back an error deferred so a short read can return the bytes it already
/// took. Fills an empty slot only: [`take_rx_error`] clears the slot, and the
/// interrupt handler may have stored a newer line-status error before this runs.
/// Overwriting that error would drop it.
fn restore_rx_error<T: Instance>(error: Error) {
    save_rx_error::<T>(error);
}

/// Read LSR without losing an RX error when the caller only wants TX status.
fn read_lsr_for_tx<T: Instance>() -> pac::uart::regs::Lsr {
    let lsr = T::regs().lsr().read();
    if let Some(error) = lsr_error(lsr) {
        save_rx_error::<T>(error);
    }
    lsr
}

fn configure<T: Instance, const REMAP: u8>(config: &Config, enable_tx: bool) -> Result<(), ConfigError> {
    // DIV = 1, so DL = Fsys / (8 * baud), rounded to the nearest integer.
    // The datasheet's reference clock is Fsys * 2 / DIV divided by 16 * DL.
    // Validate before touching the mux or resetting a working peripheral.
    let baud = config.baudrate as u64;
    if baud == 0 {
        return Err(ConfigError::BaudrateTooHigh);
    }
    let denominator = 8 * baud;
    let dl = (T::frequency().0 as u64 + denominator / 2) / denominator;
    if dl == 0 {
        return Err(ConfigError::BaudrateTooHigh);
    }
    if dl > u16::MAX as u64 {
        return Err(ConfigError::BaudrateTooLow);
    }
    // The realised rate is Fsys / (8 * DL). More than two percent off is
    // refused rather than programmed: 115200 at the 5.333 MHz reset clock lands
    // at about 111.1 kbaud, which an external receiver will not accept.
    let actual = T::frequency().0 as u64 / (8 * dl);
    let error = actual.abs_diff(baud);
    if error * 100 > baud * 2 {
        return Err(ConfigError::Baudrate);
    }

    critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });
    T::state().rx_error.store(0, Ordering::Relaxed);

    let r = T::regs();

    // Software reset, then leave every interrupt disabled. Writing RESET alone
    // also clears TXD_EN and the modem output enables.
    r.ier().write(|w| w.set_ier_reset(true));
    r.ier().write(|w| w.set_ier_txd_en(enable_tx));

    // The eight-byte FIFO is recommended over 16C450-compatible mode. Both
    // clear bits are write-one, auto-clearing.
    r.fcr().write(|w| {
        w.set_fcr_fifo_en(true);
        w.set_fcr_rx_fifo_clr(true);
        w.set_fcr_tx_fifo_clr(true);
        w.set_fcr_fifo_trig(0);
    });

    let (par_en, par_mod) = match config.parity {
        Parity::None => (false, 0),
        Parity::Odd => (true, 0b00),
        Parity::Even => (true, 0b01),
        Parity::Mark => (true, 0b10),
        Parity::Space => (true, 0b11),
    };
    r.lcr().write(|w| {
        w.set_lcr_word_sz(config.data_bits as u8);
        w.set_lcr_stop_bit(config.stop_bits as u8 != 0);
        w.set_lcr_par_en(par_en);
        w.set_lcr_par_mod(par_mod);
    });

    r.div().write(|w| w.set_div(1));
    r.dl().write(|w| w.set_dl(dl as u16));

    Ok(())
}

/// Configure a pin as UART TX or RX.
fn configure_tx_pin(pin: &AnyPin) {
    pin.set_high();
    pin.set_speed(Speed::High);
    pin.set_direction(true);
}

/// Configure an RX pad: input, pulled up.
///
/// Every constructor calls this *before* [`configure`] resets the peripheral.
/// The receiver is always enabled, so a pad that is still floating when the
/// reset lands makes it latch one 0xFF frame; configuring the pull-up first
/// keeps the start-up FIFO clean (measured: without the ordering the first
/// `blocking_read` returned 0xFF followed by shifted data).
fn configure_rx_pin(pin: &AnyPin) {
    pin.set_direction(false);
    pin.set_pull(Pull::Up);
}
