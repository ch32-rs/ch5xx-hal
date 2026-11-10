//! I2C master.
//!
//! CH585 has one controller, `I2C0`, clocked from `Fsys`. `CTRL2.FREQ` counts
//! whole megahertz and the vendor documents 2 MHz to 36 MHz, so a system clock
//! outside that range is rejected rather than truncated. Fast mode stops at
//! 400 kHz.
//!
//! The default pins are PB13 (SCL) and PB12 (SDA) on CH585, and PB15 (SCL)
//! and PB14 (SDA) on CH592. On CH585 `PIN_ALTERNATE.PIN_I2C` moves the pair to
//! PB21 and PB20; CH592 has no remap. The pins are open-drain in the
//! peripheral, so the pad is left as an input with its pull-up and the
//! controller drives the line. An external pull-up is still required.
//!
//! [`I2c::new_blocking`] polls `STAR1` and gives up after one millisecond,
//! counted from the system clock, so it does not depend on the Embassy time
//! driver. [`I2c::new`] instead runs the same byte-level sequence from the
//! `I2C0` interrupt: the handler moves each byte into or out of the caller's
//! buffer, so a busy executor cannot overrun a receive, and the future is woken
//! only when the frame ends.
//!
//! I2C has no DMA registers of its own. The async path is interrupt-driven, not
//! a DMA path.
//!
//! After roughly 100 ms without traffic the controller latches `STAR2.BUSY`
//! even though both lines are idle, and it then ignores `START`. A fresh frame
//! detects that state, resets and reprograms the block, and sends one
//! address-only start/stop pair to resynchronise the slave before the real
//! transfer. Repeated starts inside a frame are left untouched.
//!
//! A read ends the way the vendor's STM32-style routine does. The last byte is
//! NACKed, and for two or three bytes the NACK and the STOP are written while the
//! byte-transfer flag is set, inside one critical section, so the controller does
//! not clock another byte before the NACK lands.

#[cfg(feature = "embassy")]
use core::cell::UnsafeCell;
#[cfg(feature = "embassy")]
use core::future::poll_fn;
use core::marker::PhantomData;
#[cfg(feature = "embassy")]
use core::task::Poll;

use embassy_hal_internal::PeripheralType;
#[cfg(feature = "embassy")]
use embassy_sync::waitqueue::AtomicWaker;
use embedded_hal::i2c::{NoAcknowledgeSource, Operation};

use crate::gpio::{Flex, Pin, Pull};
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
    /// `STAR1.BERR`: a misplaced START or STOP on the bus.
    Bus,
    /// `STAR1.ARLO`: another master won the bus.
    ArbitrationLoss,
    /// `STAR1.AF`: the addressed device, or a written byte, was not acknowledged.
    NoAcknowledge(NoAcknowledgeSource),
    /// `STAR1.OVR`: a byte arrived before the previous one was read.
    Overrun,
    /// The bus did not reach the expected state within one millisecond.
    Timeout,
    /// The address is wider than 7 bits.
    Address,
    /// A read operation was empty, so there is no byte to NACK.
    ZeroLength,
}

/// Why a constructor refused the configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// `Fsys` is below 2 MHz or above 36 MHz, outside `CTRL2.FREQ`'s range.
    Clock,
    /// The requested rate is zero or above 400 kHz.
    Frequency,
}

impl embedded_hal::i2c::Error for Error {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        use embedded_hal::i2c::ErrorKind;
        match self {
            Error::Bus => ErrorKind::Bus,
            Error::ArbitrationLoss => ErrorKind::ArbitrationLoss,
            Error::NoAcknowledge(source) => ErrorKind::NoAcknowledge(*source),
            Error::Overrun => ErrorKind::Overrun,
            Error::Timeout => ErrorKind::Other,
            Error::Address | Error::ZeroLength => ErrorKind::Other,
        }
    }
}

/// SCL duty in fast mode. Standard mode ignores it and always divides by two.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Duty {
    /// Low time is twice the high time.
    Ratio2to1,
    /// Low time is 16/9 of the high time.
    Ratio16to9,
}

/// Master configuration.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Config {
    /// Bus rate, at most 400 kHz.
    pub frequency: Hertz,
    /// Fast-mode duty. Standard mode (100 kHz and below) does not use it.
    pub duty: Duty,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            frequency: Hertz(100_000),
            duty: Duty::Ratio2to1,
        }
    }
}

impl Config {
    /// `frequency` with a 2:1 duty. Rates of 100 kHz and below are standard mode.
    pub const fn new(frequency: Hertz) -> Self {
        Self {
            frequency,
            duty: Duty::Ratio2to1,
        }
    }
}

/// Clock fields programmed into `CTRL2`, `CKCFGR` and `RTR`.
#[derive(Clone, Copy)]
struct Timing {
    /// `CTRL2.FREQ`, whole megahertz.
    freq_mhz: u8,
    /// `CKCFGR.CCR`.
    ccr: u16,
    /// `CKCFGR.F_S`: fast mode.
    fast: bool,
    /// `CKCFGR.DUTY`: 16/9. Only meaningful in fast mode.
    duty: bool,
    /// `RTR.TRISE`, capped at the field width.
    rise: u8,
}

fn timing(input: Hertz, frequency: Hertz, duty: Duty) -> Result<Timing, ConfigError> {
    // FREQ is documented as 2..=36 MHz. Truncating a faster clock would program a
    // rate the controller cannot produce.
    let mhz = input.0 / 1_000_000;
    if !(2..=36).contains(&mhz) {
        return Err(ConfigError::Clock);
    }
    if frequency.0 == 0 || frequency.0 > 400_000 {
        return Err(ConfigError::Frequency);
    }
    let fast = frequency.0 > 100_000;
    let factor = if !fast {
        2
    } else if duty == Duty::Ratio2to1 {
        3
    } else {
        25
    };
    // Vendor formula: CCR = Fsys / (speed * factor), with a floor of 4 in
    // standard mode and 1 in fast mode.
    let mut ccr = u64::from(input.0) / (u64::from(frequency.0) * factor);
    ccr = ccr.max(if fast { 1 } else { 4 });
    if ccr > 0x0fff {
        return Err(ConfigError::Frequency);
    }
    let rise = if fast { mhz * 300 / 1000 + 1 } else { mhz + 1 };
    Ok(Timing {
        freq_mhz: mhz as u8,
        ccr: ccr as u16,
        fast,
        duty: fast && duty == Duty::Ratio16to9,
        rise: rise.min(0x3f) as u8,
    })
}

/// Blocking I2C master.
pub struct I2c<'d, T: Instance, M: Mode> {
    scl: Flex<'d>,
    sda: Flex<'d>,
    timing: Timing,
    _phantom: PhantomData<(&'d mut T, M)>,
}

impl<'d, T: Instance> I2c<'d, T, Blocking> {
    /// Create a blocking master.
    ///
    /// `REMAP` is `0` for the default pair (PB13/PB12 on CH585, PB15/PB14 on
    /// CH592) and `1` for PB21/PB20 on CH585. The pins must match: a default
    /// pin cannot be passed with the remapped value.
    pub fn new_blocking<const REMAP: u8>(
        _peri: Peri<'d, T>,
        scl: Peri<'d, impl SclPin<T, REMAP>>,
        sda: Peri<'d, impl SdaPin<T, REMAP>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let timing = timing(T::frequency(), config.frequency, config.duty)?;
        // Pull the lines up before the controller is enabled, so enabling it
        // does not sample a floating bus.
        let mut scl = Flex::new(scl.into());
        let mut sda = Flex::new(sda.into());
        scl.set_as_input(Pull::Up);
        sda.set_as_input(Pull::Up);
        critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });
        apply_timing::<T>(&timing);
        Ok(Self {
            scl,
            sda,
            timing,
            _phantom: PhantomData,
        })
    }
}

#[cfg(feature = "embassy")]
impl<'d, T: Instance> I2c<'d, T, Async> {
    /// Create an interrupt-driven master.
    ///
    /// `_irq` is the [`crate::bind_interrupts!`] binding for `I2C0`.
    pub fn new<const REMAP: u8>(
        _peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        scl: Peri<'d, impl SclPin<T, REMAP>>,
        sda: Peri<'d, impl SdaPin<T, REMAP>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let timing = timing(T::frequency(), config.frequency, config.duty)?;
        let mut scl = Flex::new(scl.into());
        let mut sda = Flex::new(sda.into());
        scl.set_as_input(Pull::Up);
        sda.set_as_input(Pull::Up);
        critical_section::with(|_| unsafe { T::set_remap(REMAP != 0) });
        apply_timing::<T>(&timing);
        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };
        Ok(Self {
            scl,
            sda,
            timing,
            _phantom: PhantomData,
        })
    }

    /// Write `bytes`, then STOP.
    pub async fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Error> {
        self.write_frame(address, bytes, true, true).await
    }

    /// Read `buffer.len()` bytes, NACKing the last one, then STOP.
    pub async fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Error> {
        self.read_frame(address, buffer, true, true, true).await
    }

    /// Write, then a repeated START, then read.
    pub async fn write_read(&mut self, address: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<(), Error> {
        if buffer.is_empty() {
            return Err(Error::ZeroLength);
        }
        self.write_frame(address, bytes, true, false).await?;
        self.read_frame(address, buffer, true, true, true).await
    }

    /// Run a transaction, mirroring [`I2c::blocking_transaction`] over interrupts.
    pub async fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        check_address(address)?;
        for operation in operations.iter() {
            if let Operation::Read(buffer) = operation {
                if buffer.is_empty() {
                    return Err(Error::ZeroLength);
                }
            }
        }

        let mut index = 0;
        while index < operations.len() {
            let read = matches!(operations[index], Operation::Read(_));
            let count = operations[index..]
                .iter()
                .take_while(|op| matches!(op, Operation::Read(_)) == read)
                .count();
            let last = index + count == operations.len();
            if read {
                for (offset, operation) in operations[index..index + count].iter_mut().enumerate() {
                    let Operation::Read(buffer) = operation else {
                        unreachable!()
                    };
                    let first = offset == 0;
                    let final_op = offset + 1 == count;
                    let stop = last && final_op;
                    self.read_frame(address, buffer, first, stop, final_op).await?;
                }
            } else {
                for (offset, operation) in operations[index..index + count].iter().enumerate() {
                    let Operation::Write(bytes) = operation else {
                        unreachable!()
                    };
                    let first = offset == 0;
                    let final_op = offset + 1 == count;
                    let stop = last && final_op;
                    self.write_frame(address, bytes, first, stop).await?;
                }
            }
            index += count;
        }
        Ok(())
    }

    async fn write_frame(&mut self, address: u8, bytes: &[u8], start: bool, stop: bool) -> Result<(), Error> {
        check_address(address)?;
        if start && self.recover_idle() {
            // A start/stop pair with no data resynchronises the slave after the
            // reset. It may NACK the address; either outcome is the point.
            let _ = self.write_frame_raw(address, &[], true, true).await;
        }
        self.write_frame_raw(address, bytes, start, stop).await
    }

    async fn write_frame_raw(&mut self, address: u8, bytes: &[u8], start: bool, stop: bool) -> Result<(), Error> {
        let state = T::state();
        let guard = AsyncTransferGuard::<T>::arm();
        if start {
            wait_stop::<T>()?;
        }
        begin_transfer::<T>(
            state,
            address,
            bytes.as_ptr() as *mut u8,
            bytes.len(),
            false,
            start,
            stop,
            true,
        );
        let result = wait_transfer::<T>(state).await;
        let _ = wait_stop::<T>();
        guard.defuse();
        result
    }

    async fn read_frame(
        &mut self,
        address: u8,
        buffer: &mut [u8],
        start: bool,
        stop: bool,
        nack: bool,
    ) -> Result<(), Error> {
        check_address(address)?;
        if buffer.is_empty() {
            return Err(Error::ZeroLength);
        }
        if start && self.recover_idle() {
            let _ = self.write_frame_raw(address, &[], true, true).await;
        }
        self.read_frame_raw(address, buffer, start, stop, nack).await
    }

    async fn read_frame_raw(
        &mut self,
        address: u8,
        buffer: &mut [u8],
        start: bool,
        stop: bool,
        nack: bool,
    ) -> Result<(), Error> {
        let state = T::state();
        let guard = AsyncTransferGuard::<T>::arm();
        if start {
            wait_stop::<T>()?;
        }
        begin_transfer::<T>(
            state,
            address,
            buffer.as_mut_ptr(),
            buffer.len(),
            true,
            start,
            stop,
            nack,
        );
        let result = wait_transfer::<T>(state).await;
        let _ = wait_stop::<T>();
        guard.defuse();
        result
    }
}

/// One step of the interrupt-driven master state machine.
#[cfg(feature = "embassy")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    WaitSb,
    WaitAddr,
    Write,
    WriteLast,
    Read,
    ReadMiddle,
    ReadBtf,
    ReadFinal,
    ReadOne,
    Done,
}

#[cfg(feature = "embassy")]
struct Engine {
    address: u8,
    buffer: *mut u8,
    len: usize,
    index: usize,
    read: bool,
    stop: bool,
    nack: bool,
    phase: Phase,
    error: Option<Error>,
}

#[cfg(feature = "embassy")]
impl Engine {
    const fn new() -> Self {
        Self {
            address: 0,
            buffer: core::ptr::null_mut(),
            len: 0,
            index: 0,
            read: false,
            stop: false,
            nack: true,
            phase: Phase::Idle,
            error: None,
        }
    }

    /// Configure the receive path once the address has been acknowledged, or
    /// when a read continues a group started by an earlier operation.
    fn begin_read<T: Instance>(&mut self) {
        let r = T::regs();
        let remaining = self.len - self.index;
        if !self.nack {
            r.ctrl1().modify(|w| w.set_ack(true));
            r.ctrl2().modify(|w| w.set_itbufen(true));
            self.phase = Phase::ReadMiddle;
        } else if remaining == 1 {
            r.ctrl1().modify(|w| {
                w.set_ack(false);
                w.set_stop(self.stop);
                w.set_start(!self.stop);
            });
            r.ctrl2().modify(|w| w.set_itbufen(true));
            self.phase = Phase::ReadOne;
        } else if remaining == 2 {
            r.ctrl1().modify(|w| w.set_ack(false));
            r.ctrl2().modify(|w| w.set_itbufen(false));
            self.phase = Phase::ReadFinal;
        } else if remaining == 3 {
            r.ctrl1().modify(|w| w.set_ack(true));
            r.ctrl2().modify(|w| w.set_itbufen(false));
            self.phase = Phase::ReadBtf;
        } else {
            r.ctrl1().modify(|w| w.set_ack(true));
            r.ctrl2().modify(|w| w.set_itbufen(true));
            self.phase = Phase::Read;
        }
    }

    /// Advance on the snapshot in `status`. Returns true when the frame ends.
    unsafe fn step<T: Instance>(&mut self, status: pac::i2c::regs::Star1) -> bool {
        let r = T::regs();
        match self.phase {
            Phase::Idle => return false,
            Phase::Done => return true,
            Phase::WaitSb => {
                if status.sb() {
                    // START clears once the controller owns the bus.
                    if r.ctrl1().read().start() || !r.star2().read().msl() {
                        self.error = Some(Error::ArbitrationLoss);
                        abort::<T>();
                        self.phase = Phase::Done;
                        return true;
                    }
                    r.datar().write(|w| w.set_datar((self.address << 1) | self.read as u8));
                    self.phase = Phase::WaitAddr;
                }
            }
            Phase::WaitAddr => {
                if status.addr() {
                    // Reading STAR2 clears ADDR.
                    let _ = r.star2().read();
                    if self.read {
                        self.begin_read::<T>();
                    } else if self.len == 0 {
                        end_frame::<T>(self.stop);
                        self.phase = Phase::Done;
                    } else {
                        r.datar().write(|w| w.set_datar(*self.buffer));
                        self.index = 1;
                        if self.index == self.len {
                            r.ctrl2().modify(|w| w.set_itbufen(false));
                            self.phase = Phase::WriteLast;
                        } else {
                            r.ctrl2().modify(|w| w.set_itbufen(true));
                            self.phase = Phase::Write;
                        }
                    }
                }
            }
            Phase::Write => {
                if status.tx_e() {
                    r.datar().write(|w| w.set_datar(*self.buffer.add(self.index)));
                    self.index += 1;
                    if self.index == self.len {
                        r.ctrl2().modify(|w| w.set_itbufen(false));
                        self.phase = Phase::WriteLast;
                    }
                }
            }
            Phase::WriteLast => {
                if status.btf() {
                    end_frame::<T>(self.stop);
                    self.phase = Phase::Done;
                }
            }
            Phase::Read => {
                if status.rx_ne() {
                    *self.buffer.add(self.index) = r.datar().read().datar();
                    self.index += 1;
                    if self.len - self.index == 3 {
                        // The tail uses BTF only, so mask the level-triggered
                        // receive source instead of re-entering every cycle.
                        r.ctrl2().modify(|w| w.set_itbufen(false));
                        self.phase = Phase::ReadBtf;
                    }
                }
            }
            Phase::ReadMiddle => {
                if status.rx_ne() {
                    *self.buffer.add(self.index) = r.datar().read().datar();
                    self.index += 1;
                    if self.index == self.len {
                        r.ctrl2().modify(|w| w.set_itbufen(false));
                        self.phase = Phase::Done;
                    }
                }
            }
            Phase::ReadBtf => {
                if status.btf() {
                    // NACK the second-to-last byte, then take the third-to-last.
                    r.ctrl1().modify(|w| w.set_ack(false));
                    *self.buffer.add(self.index) = r.datar().read().datar();
                    self.index += 1;
                    self.phase = Phase::ReadFinal;
                }
            }
            Phase::ReadFinal => {
                if status.btf() {
                    end_frame::<T>(self.stop);
                    *self.buffer.add(self.index) = r.datar().read().datar();
                    *self.buffer.add(self.index + 1) = r.datar().read().datar();
                    self.index += 2;
                    self.phase = Phase::Done;
                }
            }
            Phase::ReadOne => {
                if status.rx_ne() {
                    *self.buffer = r.datar().read().datar();
                    self.index = 1;
                    self.phase = Phase::Done;
                }
            }
        }
        self.phase == Phase::Done
    }
}

/// Per-controller engine and waker.
#[cfg(feature = "embassy")]
#[doc(hidden)]
pub struct State {
    waker: AtomicWaker,
    engine: UnsafeCell<Engine>,
}

// Access to the engine is serialised by critical sections: the future writes it
// while preparing a frame, and the interrupt handler reads it afterwards.
#[cfg(feature = "embassy")]
unsafe impl Sync for State {}

#[cfg(feature = "embassy")]
impl State {
    const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
            engine: UnsafeCell::new(Engine::new()),
        }
    }

    fn engine(&self) -> &mut Engine {
        unsafe { &mut *self.engine.get() }
    }
}

#[cfg(feature = "embassy")]
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

#[cfg(feature = "embassy")]
impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let state = T::state();
        let mut finished = false;
        critical_section::with(|_| {
            let status = T::regs().star1().read();
            let engine = state.engine();
            if status.berr() || status.arlo() || status.af() || status.ovr() {
                clear_error_bits::<T>(status);
                engine.error = Some(if status.berr() {
                    Error::Bus
                } else if status.arlo() {
                    Error::ArbitrationLoss
                } else if status.af() {
                    Error::NoAcknowledge(NoAcknowledgeSource::Unknown)
                } else {
                    Error::Overrun
                });
                abort::<T>();
                engine.phase = Phase::Done;
                finished = true;
            } else {
                finished = engine.step::<T>(status);
            }
            if finished {
                disable_events::<T>();
                engine.buffer = core::ptr::null_mut();
                engine.len = 0;
            }
        });
        if finished {
            state.waker.wake();
        }
    }
}

#[cfg(feature = "embassy")]
fn disable_events<T: Instance>() {
    T::regs().ctrl2().modify(|w| {
        w.set_itevten(false);
        w.set_iterren(false);
        w.set_itbufen(false);
    });
}

#[cfg(feature = "embassy")]
fn end_frame<T: Instance>(stop: bool) {
    T::regs().ctrl1().modify(|w| {
        if stop {
            w.set_stop(true);
        } else {
            w.set_start(true);
        }
    });
}

#[cfg(feature = "embassy")]
#[allow(clippy::too_many_arguments)]
fn begin_transfer<T: Instance>(
    state: &'static State,
    address: u8,
    buffer: *mut u8,
    len: usize,
    read: bool,
    start: bool,
    stop: bool,
    nack: bool,
) {
    let r = T::regs();
    critical_section::with(|_| {
        let engine = state.engine();
        engine.address = address;
        engine.buffer = buffer;
        engine.len = len;
        engine.index = 0;
        engine.read = read;
        engine.stop = stop;
        engine.nack = nack;
        engine.error = None;
        engine.phase = Phase::Idle;
        // Drop a stale error from an earlier transfer before enabling events.
        let status = r.star1().read();
        clear_error_bits::<T>(status);
        r.ctrl2().modify(|w| w.set_itbufen(false));
        // POS selects the two-byte NACK position and must be programmed before
        // the address is sent, as in the blocking path.
        r.ctrl1().modify(|w| w.set_pos(read && nack && len == 2));
        if start {
            if !(r.ctrl1().read().start() || r.star1().read().sb()) {
                r.ctrl1().modify(|w| {
                    w.set_ack(true);
                    w.set_start(true);
                });
            }
            engine.phase = Phase::WaitSb;
        } else if read {
            engine.begin_read::<T>();
        } else if len == 0 {
            end_frame::<T>(stop);
            engine.phase = Phase::Done;
        } else {
            r.ctrl2().modify(|w| w.set_itbufen(true));
            engine.phase = Phase::Write;
        }
        r.ctrl2().modify(|w| {
            w.set_itevten(true);
            w.set_iterren(true);
        });
    });
}

#[cfg(feature = "embassy")]
async fn wait_transfer<T: Instance>(state: &'static State) -> Result<(), Error> {
    poll_fn(|cx| {
        state.waker.register(cx.waker());
        critical_section::with(|_| {
            let engine = state.engine();
            if let Some(error) = engine.error.take() {
                return Poll::Ready(Err(error));
            }
            if engine.phase == Phase::Done {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
    })
    .await
}

/// Releases the bus and disables the event sources if a frame future is dropped.
#[cfg(feature = "embassy")]
struct AsyncTransferGuard<T: Instance> {
    armed: bool,
    _instance: PhantomData<T>,
}

#[cfg(feature = "embassy")]
impl<T: Instance> AsyncTransferGuard<T> {
    fn arm() -> Self {
        Self {
            armed: true,
            _instance: PhantomData,
        }
    }

    fn defuse(mut self) {
        self.armed = false;
    }
}

#[cfg(feature = "embassy")]
impl<T: Instance> Drop for AsyncTransferGuard<T> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        disable_events::<T>();
        abort::<T>();
        let state = T::state();
        critical_section::with(|_| {
            let engine = state.engine();
            engine.phase = Phase::Idle;
            engine.buffer = core::ptr::null_mut();
            engine.len = 0;
            engine.error = None;
        });
    }
}

impl<'d, T: Instance, M: Mode> I2c<'d, T, M> {
    fn regs() -> pac::i2c::I2c {
        T::regs()
    }

    /// Clear a `BUSY` flag that latched on an idle bus.
    ///
    /// After roughly 100 ms without traffic this controller reports
    /// `STAR2.BUSY` even though SCL and SDA are both high, and it ignores
    /// `START` while that bit is set. Only a software reset clears it, and the
    /// reset also clears the timing registers, so program them again. A transfer
    /// that still owns the bus, or one with a start or stop pending, is left
    /// alone.
    fn recover_idle(&self) -> bool {
        let regs = Self::regs();
        let ctrl1 = regs.ctrl1().read();
        let star2 = regs.star2().read();
        if star2.msl() || !(star2.busy() || ctrl1.start() || ctrl1.stop()) {
            return false;
        }
        apply_timing::<T>(&self.timing);
        // `SWRST` releases SCL and SDA. Wait for the pull-ups to restore both
        // lines before the next start.
        let limit = deadline();
        while !(self.scl.is_high() && self.sda.is_high()) {
            if timed_out(limit) {
                break;
            }
        }
        true
    }

    /// Write `bytes`, then STOP.
    pub fn blocking_write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Error> {
        self.write_bytes(address, bytes, Frame::StartAndStop)
    }

    /// Read `buffer.len()` bytes, NACKing the last one, then STOP.
    pub fn blocking_read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Error> {
        self.read_bytes(address, buffer, Frame::StartAndStop)
    }

    /// Write, then a repeated START, then read.
    pub fn blocking_write_read(&mut self, address: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<(), Error> {
        if buffer.is_empty() {
            return Err(Error::ZeroLength);
        }
        self.write_bytes(address, bytes, Frame::Start)?;
        self.read_bytes(address, buffer, Frame::StartAndStop)
    }

    /// Run a transaction.
    ///
    /// Consecutive reads are one transfer: only its last byte is NACKed.
    /// Consecutive writes are sent without a repeated START between them. A STOP
    /// follows the last operation.
    pub fn blocking_transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        check_address(address)?;
        for operation in operations.iter() {
            if let Operation::Read(buffer) = operation {
                if buffer.is_empty() {
                    return Err(Error::ZeroLength);
                }
            }
        }

        let mut index = 0;
        while index < operations.len() {
            let read = matches!(operations[index], Operation::Read(_));
            let count = operations[index..]
                .iter()
                .take_while(|op| matches!(op, Operation::Read(_)) == read)
                .count();
            let last = index + count == operations.len();
            if read {
                self.read_group(address, &mut operations[index..index + count], last)?;
            } else {
                for (offset, operation) in operations[index..index + count].iter().enumerate() {
                    let Operation::Write(bytes) = operation else {
                        unreachable!()
                    };
                    let frame = match (offset == 0, last && offset + 1 == count) {
                        (true, true) => Frame::StartAndStop,
                        (true, false) => Frame::Start,
                        (false, true) => Frame::Stop,
                        (false, false) => Frame::Middle,
                    };
                    self.write_bytes(address, bytes, frame)?;
                }
            }
            index += count;
        }
        Ok(())
    }

    fn write_bytes(&mut self, address: u8, bytes: &[u8], frame: Frame) -> Result<(), Error> {
        check_address(address)?;
        if frame.start() && self.recover_idle() {
            // Resynchronise the slave after the reset with a start/stop pair.
            let _ = self.write_bytes_raw(address, &[], Frame::StartAndStop);
        }
        self.write_bytes_raw(address, bytes, frame)
    }

    fn write_bytes_raw(&mut self, address: u8, bytes: &[u8], frame: Frame) -> Result<(), Error> {
        let guard = TransferGuard::<T>::arm();
        if frame.start() {
            start::<T>(false)?;
            wait::<T>(|status| status.sb())?;
            // START is cleared by hardware once it owns the bus. If it is still
            // set, or the controller is not the master, someone else won it.
            let regs = Self::regs();
            if regs.ctrl1().read().start() || !regs.star2().read().msl() {
                return Err(Error::ArbitrationLoss);
            }
            regs.datar().write(|w| w.set_datar(address << 1));
            wait::<T>(|status| status.addr())?;
            let _ = regs.star2().read();
        }
        for byte in bytes {
            wait::<T>(|status| status.tx_e())?;
            Self::regs().datar().write(|w| w.set_datar(*byte));
            wait::<T>(|status| status.btf())?;
        }
        if frame.stop() {
            Self::regs().ctrl1().modify(|w| w.set_stop(true));
            wait_stop::<T>()?;
        }
        guard.defuse();
        Ok(())
    }

    fn read_bytes(&mut self, address: u8, buffer: &mut [u8], frame: Frame) -> Result<(), Error> {
        let length = buffer.len();
        self.read_into(address, buffer.iter_mut(), length, frame)
    }

    fn read_group(&mut self, address: u8, operations: &mut [Operation<'_>], last: bool) -> Result<(), Error> {
        let length = operations
            .iter()
            .map(|op| match op {
                Operation::Read(buffer) => buffer.len(),
                Operation::Write(_) => 0,
            })
            .sum();
        let bytes = operations.iter_mut().flat_map(|op| match op {
            Operation::Read(buffer) => buffer.iter_mut(),
            Operation::Write(_) => unreachable!(),
        });
        let frame = if last { Frame::StartAndStop } else { Frame::Start };
        self.read_into(address, bytes, length, frame)
    }

    fn read_into<'a>(
        &mut self,
        address: u8,
        buffer: impl Iterator<Item = &'a mut u8>,
        remaining: usize,
        frame: Frame,
    ) -> Result<(), Error> {
        check_address(address)?;
        if remaining == 0 {
            return Err(Error::ZeroLength);
        }
        if frame.start() && self.recover_idle() {
            let _ = self.write_bytes_raw(address, &[], Frame::StartAndStop);
        }
        self.read_into_raw(address, buffer, remaining, frame)
    }

    fn read_into_raw<'a>(
        &mut self,
        address: u8,
        mut buffer: impl Iterator<Item = &'a mut u8>,
        mut remaining: usize,
        frame: Frame,
    ) -> Result<(), Error> {
        let guard = TransferGuard::<T>::arm();
        let regs = Self::regs();
        // POS makes a two-byte read NACK the second byte instead of the third.
        regs.ctrl1().modify(|w| w.set_pos(remaining == 2 && frame.nack()));

        if frame.start() {
            start::<T>(true)?;
            wait::<T>(|status| status.sb())?;
            if regs.ctrl1().read().start() || !regs.star2().read().msl() {
                return Err(Error::ArbitrationLoss);
            }
            regs.datar().write(|w| w.set_datar((address << 1) | 1));
            wait::<T>(|status| status.addr())?;
            critical_section::with(|_| {
                if remaining <= 2 && frame.nack() {
                    regs.ctrl1().modify(|w| w.set_ack(false));
                }
                // Reading STAR2 clears ADDR. For a single byte the STOP has to
                // be set before that read returns, or the controller clocks one
                // more byte.
                let _ = regs.star2().read();
                if remaining == 1 && frame.stop() {
                    regs.ctrl1().modify(|w| w.set_stop(true));
                } else if remaining == 1 && frame.nack() {
                    regs.ctrl1().modify(|w| w.set_start(true));
                }
            });
        }

        if !frame.nack() {
            for byte in buffer {
                *byte = recv::<T>()?;
            }
        } else if remaining == 1 {
            *buffer.next().unwrap() = recv::<T>()?;
        } else {
            while remaining > 3 {
                *buffer.next().unwrap() = recv::<T>()?;
                remaining -= 1;
            }
            wait::<T>(|status| status.btf())?;
            if remaining == 3 {
                regs.ctrl1().modify(|w| w.set_ack(false));
                *buffer.next().unwrap() = regs.datar().read().datar();
                wait::<T>(|status| status.btf())?;
            }
            critical_section::with(|_| {
                if frame.stop() {
                    regs.ctrl1().modify(|w| w.set_stop(true));
                } else {
                    regs.ctrl1().modify(|w| w.set_start(true));
                }
                *buffer.next().unwrap() = regs.datar().read().datar();
                *buffer.next().unwrap() = regs.datar().read().datar();
            });
        }

        if frame.stop() {
            wait_stop::<T>()?;
            regs.ctrl1().modify(|w| w.set_pos(false));
        }
        guard.defuse();
        Ok(())
    }
}

impl<'d, T: Instance, M: Mode> Drop for I2c<'d, T, M> {
    fn drop(&mut self) {
        #[cfg(feature = "embassy")]
        {
            disable_events::<T>();
            T::Interrupt::disable();
        }
        let regs = T::regs();
        regs.ctrl1().modify(|w| w.set_pe(false));
        self.scl.set_as_input(Pull::Up);
        self.sda.set_as_input(Pull::Up);
    }
}

/// How a transfer joins its neighbours.
#[derive(Clone, Copy)]
enum Frame {
    /// START before, STOP after.
    StartAndStop,
    /// START before, no STOP.
    Start,
    /// No START, STOP after.
    Stop,
    /// Neither. Continues the previous byte stream.
    Middle,
}

impl Frame {
    fn start(self) -> bool {
        matches!(self, Frame::StartAndStop | Frame::Start)
    }

    fn stop(self) -> bool {
        matches!(self, Frame::StartAndStop | Frame::Stop)
    }

    fn nack(self) -> bool {
        matches!(self, Frame::StartAndStop | Frame::Start | Frame::Stop)
    }
}

/// Releases the bus if a transfer returns early.
struct TransferGuard<T: Instance> {
    armed: bool,
    _instance: PhantomData<T>,
}

impl<T: Instance> TransferGuard<T> {
    fn arm() -> Self {
        Self {
            armed: true,
            _instance: PhantomData,
        }
    }

    fn defuse(mut self) {
        self.armed = false;
    }
}

impl<T: Instance> Drop for TransferGuard<T> {
    fn drop(&mut self) {
        if self.armed {
            abort::<T>();
        }
    }
}

fn apply_timing<T: Instance>(timing: &Timing) {
    let regs = T::regs();
    // Disable the peripheral first so the reset and the timing writes happen
    // with both pins released, then pulse the software reset before programming
    // the clock.
    regs.ctrl1().modify(|w| w.set_pe(false));
    regs.ctrl1().modify(|w| w.set_swrst(true));
    regs.ctrl1().modify(|w| w.set_swrst(false));
    regs.ctrl2().modify(|w| w.set_freq(timing.freq_mhz));
    regs.ckcfgr().write(|w| {
        w.set_ccr(timing.ccr);
        w.set_duty(timing.duty);
        w.set_f_s(timing.fast);
    });
    regs.rtr().write(|w| w.set_trise(timing.rise));
    regs.ctrl1().modify(|w| {
        w.set_pe(true);
        w.set_ack(true);
    });
}

fn start<T: Instance>(ack: bool) -> Result<(), Error> {
    wait_stop::<T>()?;
    let regs = T::regs();
    if regs.ctrl1().read().start() || regs.star1().read().sb() {
        return Ok(());
    }
    regs.ctrl1().modify(|w| {
        w.set_start(true);
        w.set_ack(ack);
    });
    Ok(())
}

fn wait_stop<T: Instance>() -> Result<(), Error> {
    let deadline = deadline();
    while T::regs().ctrl1().read().stop() {
        if timed_out(deadline) {
            return Err(Error::Timeout);
        }
    }
    Ok(())
}

fn wait<T: Instance>(ready: impl Fn(pac::i2c::regs::Star1) -> bool) -> Result<(), Error> {
    let deadline = deadline();
    loop {
        let status = status::<T>()?;
        if ready(status) {
            return Ok(());
        }
        if timed_out(deadline) {
            return Err(Error::Timeout);
        }
    }
}

fn recv<T: Instance>() -> Result<u8, Error> {
    let deadline = deadline();
    loop {
        status::<T>()?;
        if T::regs().star1().read().rx_ne() {
            return Ok(T::regs().datar().read().datar());
        }
        if timed_out(deadline) {
            return Err(Error::Timeout);
        }
    }
}

/// Read `STAR1` and clear a latched error.
///
/// The error bits are RW0. The PAC exposes the register as read-only, so the
/// clear is a direct write of every bit except the ones observed set.
fn clear_error_bits<T: Instance>(status: pac::i2c::regs::Star1) {
    let errors = status.0 & ERROR_BITS;
    if errors != 0 {
        unsafe {
            core::ptr::write_volatile(T::regs().star1().as_ptr(), pac::i2c::regs::Star1(!errors));
        }
    }
}

fn status<T: Instance>() -> Result<pac::i2c::regs::Star1, Error> {
    let status = T::regs().star1().read();
    clear_error_bits::<T>(status);
    if status.berr() {
        Err(Error::Bus)
    } else if status.arlo() {
        Err(Error::ArbitrationLoss)
    } else if status.af() {
        Err(Error::NoAcknowledge(NoAcknowledgeSource::Unknown))
    } else if status.ovr() {
        Err(Error::Overrun)
    } else {
        Ok(status)
    }
}

/// `BERR`, `ARLO`, `AF` and `OVR`.
const ERROR_BITS: u16 = (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11);

fn abort<T: Instance>() {
    let regs = T::regs();
    if regs.star2().read().msl() {
        regs.ctrl1().modify(|w| w.set_stop(true));
    }
}

fn check_address(address: u8) -> Result<(), Error> {
    if address > 0x7f {
        Err(Error::Address)
    } else {
        Ok(())
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

pub(crate) trait SealedInstance {
    fn regs() -> pac::i2c::I2c;

    /// Set `PIN_ALTERNATE.PIN_I2C`.
    ///
    /// # Safety
    ///
    /// The register is shared with every other peripheral, so the caller holds a
    /// critical section.
    unsafe fn set_remap(remapped: bool);

    #[cfg(feature = "embassy")]
    fn state() -> &'static State;
}

/// An I2C controller.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType {
    /// Interrupt raised by this controller.
    type Interrupt: interrupt::typelevel::Interrupt;

    /// Clock feeding the controller. I2C0 runs at `Fsys`.
    fn frequency() -> Hertz {
        crate::rcc::clocks().sysclk
    }
}

impl SealedInstance for peripherals::I2C0 {
    fn regs() -> pac::i2c::I2c {
        pac::I2C0
    }

    unsafe fn set_remap(remapped: bool) {
        #[cfg(pin_i2c)]
        pac::SYS.pin_alternate().modify(|w| w.set_pin_i2c(remapped));
        #[cfg(not(pin_i2c))]
        let _ = remapped;
    }

    #[cfg(feature = "embassy")]
    fn state() -> &'static State {
        static STATE: State = State::new();
        &STATE
    }
}

impl Instance for peripherals::I2C0 {
    type Interrupt = interrupt::typelevel::I2C0;
}

mod pin_sealed {
    pub trait Scl {}
    pub trait Sda {}
}

/// SCL pin for `T` at remap `REMAP`.
pub trait SclPin<T: Instance, const REMAP: u8>: Pin + pin_sealed::Scl {}

/// SDA pin for `T` at remap `REMAP`.
pub trait SdaPin<T: Instance, const REMAP: u8>: Pin + pin_sealed::Sda {}

macro_rules! impl_pins {
    ($inst:ident, $remap:literal, scl: [$($scl:ident),* $(,)?], sda: [$($sda:ident),* $(,)?]) => {
        $(impl pin_sealed::Scl for peripherals::$scl {})*
        $(impl SclPin<peripherals::$inst, $remap> for peripherals::$scl {})*
        $(impl pin_sealed::Sda for peripherals::$sda {})*
        $(impl SdaPin<peripherals::$inst, $remap> for peripherals::$sda {})*
    };
}

// Datasheet table 7-8. On CH585 `PIN_I2C` clear is PB13/PB12 and set is
// PB21/PB20. CH592 and CH595 route I2C0 to PB15 (SCL) and PB14 (SDA) with no
// remap field, so only that pair exists there.
#[cfg(not(any(ch572, ch592, ch595)))]
impl_pins!(I2C0, 0, scl: [PB13], sda: [PB12]);
#[cfg(pin_i2c)]
impl_pins!(I2C0, 1, scl: [PB21], sda: [PB20]);
#[cfg(any(ch592, ch595))]
impl_pins!(I2C0, 0, scl: [PB15], sda: [PB14]);
// CH572/CH570 use PA8 (SCL) and PA9 (SDA). `PIN_ALTERNATE_H.I2C_PIN` selects
// other routes, which the single-bit remap API does not model.
#[cfg(ch572)]
impl_pins!(I2C0, 0, scl: [PA8], sda: [PA9]);

// --- embedded-hal ----------------------------------------------------------

impl<'d, T: Instance, M: Mode> embedded_hal::i2c::ErrorType for I2c<'d, T, M> {
    type Error = Error;
}

impl<'d, T: Instance, M: Mode> embedded_hal::i2c::I2c for I2c<'d, T, M> {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        self.blocking_transaction(address, operations)
    }
}

#[cfg(feature = "embassy")]
impl<'d, T: Instance> embedded_hal_async::i2c::I2c for I2c<'d, T, Async> {
    async fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        self.transaction(address, operations).await
    }
}
