//! I2C master.
//!
//! CH585 has one controller, `I2C0`, clocked from `Fsys`. `CTRL2.FREQ` counts
//! whole megahertz and the vendor documents 2 MHz to 36 MHz, so a system clock
//! outside that range is rejected rather than truncated. Fast mode stops at
//! 400 kHz.
//!
//! The default pins are PB13 (SCL) and PB12 (SDA). `PIN_ALTERNATE.PIN_I2C`
//! moves the pair to PB21 and PB20. The pins are open-drain in the peripheral,
//! so the pad is left as an input with its pull-up and the controller drives the
//! line. An external pull-up is still required.
//!
//! Only the blocking master is implemented. A transfer polls `STAR1` and gives up
//! after one millisecond, counted from the system clock, so it does not depend on
//! the Embassy time driver. The controller has no transfer-complete interrupt of
//! its own; the event and buffer enables are left for an async driver.
//!
//! A read ends the way the vendor's STM32-style routine does. The last byte is
//! NACKed, and for two or three bytes the NACK and the STOP are written while the
//! byte-transfer flag is set, inside one critical section, so the controller does
//! not clock another byte before the NACK lands.

use core::marker::PhantomData;

use embassy_hal_internal::PeripheralType;
use embedded_hal::i2c::{NoAcknowledgeSource, Operation};

use crate::gpio::{Flex, Pin, Pull};
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
    _phantom: PhantomData<(&'d mut T, M)>,
}

impl<'d, T: Instance> I2c<'d, T, Blocking> {
    /// Create a blocking master.
    ///
    /// `REMAP` is `0` for PB13/PB12 and `1` for PB21/PB20. The pins must match:
    /// a default pin cannot be passed with the remapped value.
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
            _phantom: PhantomData,
        })
    }
}

impl<'d, T: Instance, M: Mode> I2c<'d, T, M> {
    fn regs() -> pac::i2c::I2c {
        T::regs()
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
        mut buffer: impl Iterator<Item = &'a mut u8>,
        mut remaining: usize,
        frame: Frame,
    ) -> Result<(), Error> {
        check_address(address)?;
        if remaining == 0 {
            return Err(Error::ZeroLength);
        }
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
    // The vendor pulses the software reset before programming the clock, so a
    // half-finished transfer cannot keep the old divider.
    regs.ctrl1().modify(|w| w.set_swrst(true));
    regs.ctrl1().modify(|w| w.set_swrst(false));
    regs.ctrl1().modify(|w| w.set_pe(false));
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
fn status<T: Instance>() -> Result<pac::i2c::regs::Star1, Error> {
    let regs = T::regs();
    let status = regs.star1().read();
    let errors = status.0 & ERROR_BITS;
    if errors != 0 {
        unsafe {
            core::ptr::write_volatile(regs.star1().as_ptr(), pac::i2c::regs::Star1(!errors));
        }
    }
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
        pac::SYS.pin_alternate().modify(|w| w.set_pin_i2c(remapped));
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

// Datasheet table 7-8. `PIN_I2C` clear is PB13/PB12, set is PB21/PB20.
impl_pins!(I2C0, 0, scl: [PB13], sda: [PB12]);
impl_pins!(I2C0, 1, scl: [PB21], sda: [PB20]);

// --- embedded-hal ----------------------------------------------------------

impl<'d, T: Instance, M: Mode> embedded_hal::i2c::ErrorType for I2c<'d, T, M> {
    type Error = Error;
}

impl<'d, T: Instance, M: Mode> embedded_hal::i2c::I2c for I2c<'d, T, M> {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        self.blocking_transaction(address, operations)
    }
}
