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
//! Only blocking mode is implemented so far. The receiver is always enabled by
//! the peripheral; `TXD_EN` gates the transmit pin.

use core::fmt;
use core::marker::PhantomData;

use crate::gpio::{AnyPin, Pin, Pull, SealedPin, Speed};
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

pub(crate) trait SealedInstance {
    fn regs() -> pac::uart::UartFull;

    /// Set the `PIN_ALTERNATE` bit that selects the remapped pins.
    ///
    /// # Safety
    ///
    /// The remap register is shared with every other peripheral, so the caller
    /// must hold a critical section.
    unsafe fn set_remap(remapped: bool);
}

macro_rules! impl_instance {
    ($inst:ident, $setter:ident) => {
        impl SealedInstance for peripherals::$inst {
            #[inline]
            fn regs() -> pac::uart::UartFull {
                pac::$inst
            }

            #[inline]
            unsafe fn set_remap(remapped: bool) {
                pac::SYS.pin_alternate().modify(|w| w.$setter(remapped));
            }
        }

        impl Instance for peripherals::$inst {
            type Interrupt = interrupt::typelevel::$inst;
        }
    };
}

impl_instance!(UART0, set_pin_uart0);
impl_instance!(UART1, set_pin_uart1);
impl_instance!(UART2, set_pin_uart2);
impl_instance!(UART3, set_pin_uart3);

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
    _tx: Peri<'d, AnyPin>,
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
        let r = T::regs();
        while !r.lsr().read().lsr_tx_all_emp() {}
        Ok(())
    }

    fn new_inner(tx: Peri<'d, AnyPin>) -> Self {
        Self {
            _phantom: PhantomData,
            _tx: tx,
        }
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
    pub fn blocking_read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        let r = T::regs();
        for byte in buffer {
            loop {
                let lsr = r.lsr().read();
                if lsr.lsr_over_err() {
                    return Err(Error::Overrun);
                }
                if lsr.lsr_par_err() {
                    return Err(Error::Parity);
                }
                if lsr.lsr_frame_err() {
                    return Err(Error::Framing);
                }
                if lsr.lsr_break_err() {
                    return Err(Error::Break);
                }
                if lsr.lsr_data_rdy() {
                    *byte = r.rbr().read().rbr();
                    break;
                }
            }
        }
        Ok(())
    }

    fn new_inner(rx: Peri<'d, AnyPin>) -> Self {
        Self {
            _phantom: PhantomData,
            _rx: rx,
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
        configure::<T, REMAP>(&config, false)?;
        let rx = rx.into();
        configure_rx_pin(&rx);
        Ok(Self::new_inner(rx))
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

    /// Split into independent transmitter and receiver halves.
    pub fn split(self) -> (UartTx<'d, T, M>, UartRx<'d, T, M>) {
        (self.tx, self.rx)
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
        configure::<T, REMAP>(&config, true)?;
        let rx = rx.into();
        let tx = tx.into();
        configure_rx_pin(&rx);
        configure_tx_pin(&tx);
        Ok(Self {
            tx: UartTx::new_inner(tx),
            rx: UartRx::new_inner(rx),
        })
    }
}

impl<'d, T: Instance, M: Mode> fmt::Write for Uart<'d, T, M> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.blocking_write(s.as_bytes()).map_err(|_| fmt::Error)
    }
}

fn configure<T: Instance, const REMAP: u8>(config: &Config, enable_tx: bool) -> Result<(), ConfigError> {
    critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });

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

    // DIV = 1, so DL = Fsys / (8 * baud), rounded to the nearest integer.
    // The datasheet's reference clock is Fsys * 2 / DIV divided by 16 * DL.
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

fn configure_rx_pin(pin: &AnyPin) {
    pin.set_direction(false);
    pin.set_pull(Pull::Up);
}
