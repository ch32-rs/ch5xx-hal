//! GPIO
//!
//! Unlike the CH32 families there is **one** GPIO block, `pac::GPIO`, and each
//! port is a group of registers inside it (`PA_DIR`/`PB_DIR`, `PA_OUT`/`PB_OUT`,
//! …) rather than a peripheral of its own. A pin is therefore identified by
//! (port, bit) and the driver dispatches on the port letter.
//!
//! Per-port register set (CH585, offsets from `0x4000_1000`): interrupt enable,
//! mode and flag for PA/PB, then `DIR`, `PIN`, `OUT`, `CLR`, `PU`, `PD_DRV` and
//! `SET`. `CLR`/`SET` are write-1-to-act, so they are the atomic way to change
//! one pin.
//!
//! Power-on state: every pin is a floating input.
//!
//! # Port interrupts
//!
//! There is no per-pin EXTI. Each port has one interrupt (`GPIO_A`, `GPIO_B`)
//! and a 16-bit enable, mode and flag register, one bit per pin. [`Input`] waits
//! on a pin through that shared line. With the `embassy` feature, `crate::init`
//! claims both port vectors, the same way the time driver claims its counter, so
//! a wait needs no separate binding.
//!
//! A pin is either level- or edge-sensitive, and `Px_OUT` selects which one:
//! high/rising when the bit is set, low/falling when it is clear. Both edges of
//! one pin cannot be armed together. The handler masks a pin that fired and the
//! waiter re-enables it, because a level that is still present would otherwise
//! re-enter the handler until the executor runs.
//!
//! On CH585 the flag registers only cover PA0–PA15 and PB0–PB15.
//! `PB_INT_*[9:8]` follows `SYS.PIN_ALTERNATE.PIN_INTX`: clear selects PB8/PB9,
//! set selects PB22/PB23. PB16–PB21 have no interrupt source. `PIN_CONFIG`
//! `.PB16_8_SEL` is a separate select and is left untouched.

use core::convert::Infallible;
use core::future::Future;
use core::marker::PhantomData;
#[cfg(feature = "embassy")]
use core::task::Poll;

use critical_section::CriticalSection;
#[cfg(feature = "embassy")]
use embassy_sync::waitqueue::AtomicWaker;

use crate::{impl_peripheral, pac, peripherals, Peri};

/// Output drive strength.
///
/// CH5xx folds the drive strength into `Px_PD_DRV`: for an output, a 0 selects
/// 5 mA and a 1 selects 20 mA.
#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
pub enum Speed {
    /// 5 mA
    #[default]
    Low,
    /// 20 mA
    High,
}

/// Pull setting for an input.
#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
pub enum Pull {
    #[default]
    None,
    Up,
    Down,
}

/// Logic level.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Level {
    Low,
    High,
}

/// What makes a pin raise its port interrupt.
///
/// The hardware stores one choice per pin: level or edge, and one polarity. A
/// pin cannot watch both edges at once.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum InterruptTrigger {
    /// The pin reads low.
    LowLevel,
    /// The pin reads high.
    HighLevel,
    /// The pin falls.
    FallingEdge,
    /// The pin rises.
    RisingEdge,
}

impl From<bool> for Level {
    fn from(val: bool) -> Self {
        match val {
            true => Self::High,
            false => Self::Low,
        }
    }
}

impl From<Level> for bool {
    fn from(level: Level) -> bool {
        match level {
            Level::Low => false,
            Level::High => true,
        }
    }
}

/// Which physical port a pin lives on.
///
/// The CH5xx exposes `PA` and `PB` as register groups, so this is the
/// discriminator the driver switches on.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Port {
    A,
    B,
}

/// Type-erased GPIO pin.
pub struct AnyPin {
    port: Port,
    pin: u8,
}

impl AnyPin {
    /// Create a pin from its port and bit number.
    ///
    /// # Safety
    ///
    /// The caller must ensure the (port, bit) pair is not used through any
    /// other handle at the same time.
    #[inline]
    pub unsafe fn steal(port: Port, pin: u8) -> Self {
        Self { port, pin }
    }
}

impl_peripheral!(AnyPin);

#[allow(private_bounds)]
pub trait Pin: crate::PeripheralType + Into<AnyPin> + SealedPin + Sized + 'static {
    /// Bit number within the port.
    #[inline]
    fn pin(&self) -> u8 {
        self._pin()
    }

    /// Port the pin belongs to.
    #[inline]
    fn port(&self) -> Port {
        self._port()
    }

    /// Convert to the type-erased [`AnyPin`].
    #[inline]
    fn degrade(self) -> AnyPin {
        AnyPin {
            port: self._port(),
            pin: self._pin(),
        }
    }
}

pub(crate) trait SealedPin {
    fn _port(&self) -> Port;
    fn _pin(&self) -> u8;

    #[inline]
    fn block(&self) -> pac::gpio::Gpio {
        pac::GPIO
    }

    /// Read the input level.
    #[inline]
    fn is_high(&self) -> bool {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_pin().read().pa_pin(n),
            Port::B => self.block().pb_pin().read().pb_pin(n),
        }
    }

    /// Read the output latch.
    #[inline]
    fn is_set_high(&self) -> bool {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_out().read().pa_out(n),
            Port::B => self.block().pb_out().read().pb_out(n),
        }
    }

    /// Drive the pin high (write-1-to-set, so other pins are untouched).
    #[inline]
    fn set_high(&self) {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_set().write(|w| w.set_pa_set(n, true)),
            Port::B => self.block().pb_set().write(|w| w.set_pb_set(n, true)),
        }
    }

    /// Drive the pin low (write-1-to-clear).
    #[inline]
    fn set_low(&self) {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_clr().write(|w| w.set_pa_clr(n, true)),
            Port::B => self.block().pb_clr().write(|w| w.set_pb_clr(n, true)),
        }
    }

    /// Set the direction bit (1 = output).
    #[inline]
    fn set_direction(&self, output: bool) {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_dir().modify(|w| w.set_pa_dir(n, output)),
            Port::B => self.block().pb_dir().modify(|w| w.set_pb_dir(n, output)),
        }
    }

    /// Enable/disable the internal pull-up or pull-down.
    #[inline]
    fn set_pull(&self, pull: Pull) {
        let n = self._pin() as usize;
        match self._port() {
            Port::A => self.block().pa_pu().modify(|w| w.set_pa_pu(n, pull == Pull::Up)),
            Port::B => self.block().pb_pu().modify(|w| w.set_pb_pu(n, pull == Pull::Up)),
        }
        match self._port() {
            Port::A => self
                .block()
                .pa_pd_drv()
                .modify(|w| w.set_pa_pd_drv(n, matches!(pull, Pull::Down))),
            Port::B => self
                .block()
                .pb_pd_drv()
                .modify(|w| w.set_pb_pd_drv(n, matches!(pull, Pull::Down))),
        }
    }

    /// Set the drive strength (only meaningful for an output).
    #[inline]
    fn set_speed(&self, speed: Speed) {
        let n = self._pin() as usize;
        let high = speed == Speed::High;
        match self._port() {
            Port::A => self.block().pa_pd_drv().modify(|w| w.set_pa_pd_drv(n, high)),
            Port::B => self.block().pb_pd_drv().modify(|w| w.set_pb_pd_drv(n, high)),
        }
    }

    /// Latch the interrupt polarity into `Px_OUT`.
    ///
    /// While the pin is an input that register does not drive the pad. A set bit
    /// selects the high level or the rising edge; a clear bit selects the low
    /// level or the falling edge. The vendor routine writes `Px_OUT` for the high
    /// polarity and `Px_CLR` for the low one; both are used here so a polarity
    /// change never reads back a write-1-to-clear register.
    #[inline]
    fn set_interrupt_polarity(&self, high: bool) {
        let n = self._pin() as usize;
        match (self._port(), high) {
            (Port::A, true) => self.block().pa_out().modify(|w| w.set_pa_out(n, true)),
            (Port::A, false) => self.block().pa_clr().write(|w| w.set_pa_clr(n, true)),
            (Port::B, true) => self.block().pb_out().modify(|w| w.set_pb_out(n, true)),
            (Port::B, false) => self.block().pb_clr().write(|w| w.set_pb_clr(n, true)),
        }
    }
}

impl Pin for AnyPin {}
impl SealedPin for AnyPin {
    #[inline]
    fn _port(&self) -> Port {
        self.port
    }
    #[inline]
    fn _pin(&self) -> u8 {
        self.pin
    }
}

foreach_pin!(
    ($pin_name:ident, $port_name:ident, $port_num:expr, $pin_num:expr) => {
        impl Pin for peripherals::$pin_name {}

        impl SealedPin for peripherals::$pin_name {
            #[inline]
            fn _port(&self) -> Port {
                // `$port_num` is 0 for A, 1 for B.
                if $port_num == 0 { Port::A } else { Port::B }
            }
            #[inline]
            fn _pin(&self) -> u8 {
                $pin_num
            }
        }

        impl From<peripherals::$pin_name> for AnyPin {
            fn from(x: peripherals::$pin_name) -> Self {
                x.degrade()
            }
        }
    };
);

/// GPIO flexible pin.
pub struct Flex<'d> {
    pub(crate) pin: Peri<'d, AnyPin>,
}

impl<'d> Flex<'d> {
    /// Wrap a pin without changing its configuration.
    #[inline]
    pub fn new(pin: Peri<'d, impl Pin>) -> Self {
        Self { pin: pin.into() }
    }

    /// Configure as an input with the given pull.
    #[inline]
    pub fn set_as_input(&mut self, pull: Pull) {
        critical_section::with(|_| {
            self.pin.set_direction(false);
            self.pin.set_pull(pull);
        });
    }

    /// Configure as a push-pull output.
    ///
    /// The output level is whatever was last latched, so set the level first if
    /// it matters.
    #[inline]
    pub fn set_as_output(&mut self, speed: Speed) {
        critical_section::with(|_| {
            self.pin.set_speed(speed);
            self.pin.set_direction(true);
        });
    }

    #[inline]
    pub fn is_high(&self) -> bool {
        self.pin.is_high()
    }

    #[inline]
    pub fn is_low(&self) -> bool {
        !self.is_high()
    }

    #[inline]
    pub fn get_level(&self) -> Level {
        self.is_high().into()
    }

    #[inline]
    pub fn is_set_high(&self) -> bool {
        self.pin.is_set_high()
    }

    #[inline]
    pub fn is_set_low(&self) -> bool {
        !self.is_set_high()
    }

    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.is_set_high().into()
    }

    #[inline]
    pub fn set_high(&mut self) {
        self.pin.set_high();
    }

    #[inline]
    pub fn set_low(&mut self) {
        self.pin.set_low();
    }

    #[inline]
    pub fn set_level(&mut self, level: Level) {
        match level {
            Level::Low => self.set_low(),
            Level::High => self.set_high(),
        }
    }

    /// Toggle the output.
    #[inline]
    pub fn toggle(&mut self) {
        if self.is_set_low() {
            self.set_high()
        } else {
            self.set_low()
        }
    }
}

/// GPIO input driver.
///
/// The async methods wait on the pin's port interrupt. A port has one interrupt
/// line shared by every pin that can raise it, so two waits on the same port run
/// concurrently but two waits on the same pin do not: arming a pin replaces the
/// trigger the previous wait programmed.
pub struct Input<'d> {
    pub(crate) pin: Flex<'d>,
}

impl<'d> Input<'d> {
    /// Create an input on `pin` with the given pull.
    #[inline]
    pub fn new(pin: Peri<'d, impl Pin>, pull: Pull) -> Self {
        let mut pin = Flex::new(pin);
        pin.set_as_input(pull);
        Self { pin }
    }

    /// Is the input high?
    #[inline]
    pub fn is_high(&self) -> bool {
        self.pin.is_high()
    }

    /// Is the input low?
    #[inline]
    pub fn is_low(&self) -> bool {
        self.pin.is_low()
    }

    /// Current input level.
    #[inline]
    pub fn get_level(&self) -> Level {
        self.pin.get_level()
    }

    /// Wait until `trigger` happens.
    ///
    /// Returns immediately when a level trigger is already true, before the
    /// interrupt is armed. An edge trigger always waits for the next edge, even
    /// when the pin already sits at the target level.
    ///
    /// # Panics
    ///
    /// Panics if this pin has no interrupt source. On CH585 that is PB16–PB21.
    /// PB22 and PB23 are reachable and take the port-B flag bits shared with
    /// PB8 and PB9.
    #[cfg(feature = "embassy")]
    pub async fn wait_for(&mut self, trigger: InterruptTrigger) {
        if level_already_met(&self.pin.pin, trigger) {
            return;
        }
        PinWait::new(&self.pin.pin, trigger).await;
    }

    /// Wait until the pin reads high.
    ///
    /// Returns immediately if it already does. See [`Input::wait_for`].
    #[cfg(feature = "embassy")]
    pub async fn wait_for_high(&mut self) {
        self.wait_for(InterruptTrigger::HighLevel).await;
    }

    /// Wait until the pin reads low.
    ///
    /// Returns immediately if it already does. See [`Input::wait_for`].
    #[cfg(feature = "embassy")]
    pub async fn wait_for_low(&mut self) {
        self.wait_for(InterruptTrigger::LowLevel).await;
    }

    /// Wait for the next rising edge. See [`Input::wait_for`].
    #[cfg(feature = "embassy")]
    pub async fn wait_for_rising_edge(&mut self) {
        self.wait_for(InterruptTrigger::RisingEdge).await;
    }

    /// Wait for the next falling edge. See [`Input::wait_for`].
    #[cfg(feature = "embassy")]
    pub async fn wait_for_falling_edge(&mut self) {
        self.wait_for(InterruptTrigger::FallingEdge).await;
    }
}

/// GPIO output driver.
pub struct Output<'d> {
    pub(crate) pin: Flex<'d>,
}

impl<'d> Output<'d> {
    /// Create an output on `pin`, driving `initial_output` at `speed`.
    #[inline]
    pub fn new(pin: Peri<'d, impl Pin>, initial_output: Level, speed: Speed) -> Self {
        let mut pin = Flex::new(pin);
        match initial_output {
            Level::High => pin.set_high(),
            Level::Low => pin.set_low(),
        }
        pin.set_as_output(speed);
        Self { pin }
    }

    #[inline]
    pub fn set_high(&mut self) {
        self.pin.set_high();
    }

    #[inline]
    pub fn set_low(&mut self) {
        self.pin.set_low();
    }

    #[inline]
    pub fn set_level(&mut self, level: Level) {
        self.pin.set_level(level)
    }

    #[inline]
    pub fn is_set_high(&self) -> bool {
        self.pin.is_set_high()
    }

    #[inline]
    pub fn is_set_low(&self) -> bool {
        self.pin.is_set_low()
    }

    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.pin.get_output_level()
    }

    /// Toggle the output.
    #[inline]
    pub fn toggle(&mut self) {
        self.pin.toggle();
    }
}

/// Enable the GPIO block.
///
/// There is no peripheral clock gate to turn on — the CH5xx GPIO is always
/// clocked. All this does is keep the same call shape as ch32-hal so `init` can
/// treat it uniformly.
pub(crate) unsafe fn init(_cs: CriticalSection) {
    // Nothing to program: the CH5xx GPIO block is always clocked and every pin
    // comes out of reset as a floating input. Kept so `init` has the same shape
    // as ch32-hal's. Port interrupts stay masked until a wait arms its own pin.
    //
    // The two port vectors have no other owner, so the embassy build claims them
    // here. A pin's enable bit stays clear until a wait arms it, and a disabled
    // source never asserts the line.
    #[cfg(feature = "embassy")]
    {
        use crate::interrupt::InterruptExt;
        pac::Interrupt::GPIO_A.set_priority(PORT_INTERRUPT_PRIORITY);
        pac::Interrupt::GPIO_B.set_priority(PORT_INTERRUPT_PRIORITY);
        unsafe {
            pac::Interrupt::GPIO_A.enable();
            pac::Interrupt::GPIO_B.enable();
        }
    }
}

// --- port interrupts -------------------------------------------------------

/// Priority of the two port interrupts: below the time driver, above ordinary
/// peripheral work. Only bits `[7:5]` exist on this core, so this is level 5 of
/// the eight usable ones. See [`crate::embassy::TIME_DRIVER_PRIORITY`].
#[cfg(feature = "embassy")]
const PORT_INTERRUPT_PRIORITY: qingke::interrupt::Priority = qingke::interrupt::Priority::P10;

/// Both port vectors. `qingke-rt` needs a literal name, so the handlers are thin
/// wrappers over the one port dispatcher. The attribute is the runtime crate's
/// `#[interrupt]`, which the HAL re-exports.
#[cfg(feature = "embassy")]
mod irq {
    use super::{on_port, Port};

    #[crate::interrupt]
    fn GPIO_A() {
        on_port(Port::A);
    }

    #[crate::interrupt]
    fn GPIO_B() {
        on_port(Port::B);
    }
}

/// Pins of one port waiting on its shared interrupt, indexed by the flag bit.
#[cfg(feature = "embassy")]
struct PortWakers {
    wakers: [AtomicWaker; INT_BITS],
}

#[cfg(feature = "embassy")]
impl PortWakers {
    const fn new() -> Self {
        Self {
            wakers: [const { AtomicWaker::new() }; INT_BITS],
        }
    }
}

#[cfg(feature = "embassy")]
static PORT_WAKERS: [PortWakers; 2] = [PortWakers::new(), PortWakers::new()];

/// Width of `Px_INT_EN`, `Px_INT_MODE` and `Px_INT_IF`.
const INT_BITS: usize = 16;

/// Where a pin's interrupt is programmed.
///
/// `bit` is the index in the 16-bit port registers. It equals the pad number
/// except for the one documented alias below.
struct InterruptLine {
    port: Port,
    bit: u8,
    /// Set `PIN_INTX` so flag bits 9:8 follow PB22/PB23 instead of PB8/PB9.
    remap: bool,
}

/// The interrupt flag bit for `pin`, or `None` when the pad has no source.
///
/// CH585 documents one alias: `PB_INT_*[9:8]` selects PB8/PB9 or PB22/PB23
/// through `PIN_ALTERNATE.PIN_INTX`. PB16–PB21 are absent from every interrupt
/// register. `PIN_CONFIG.PB16_8_SEL` is a different select and is not touched.
fn interrupt_line(port: Port, pin: u8) -> Option<InterruptLine> {
    match (port, pin) {
        (Port::A, 0..=15) => Some(InterruptLine {
            port,
            bit: pin,
            remap: false,
        }),
        (Port::B, 0..=15) => Some(InterruptLine {
            port,
            bit: pin,
            remap: false,
        }),
        // Vendor `GPIOB_ITModeCfg` shifts these two down by 14, onto bits 8 and 9.
        (Port::B, 22) | (Port::B, 23) => Some(InterruptLine {
            port,
            bit: pin - 14,
            remap: true,
        }),
        _ => None,
    }
}

fn level_already_met(pin: &AnyPin, trigger: InterruptTrigger) -> bool {
    match trigger {
        InterruptTrigger::HighLevel => pin.is_high(),
        InterruptTrigger::LowLevel => !pin.is_high(),
        InterruptTrigger::RisingEdge | InterruptTrigger::FallingEdge => false,
    }
}

/// Arm `line` for `trigger` and clear a stale flag.
///
/// `PIN_INTX` is shared by PB8/PB9 and PB22/PB23, so it is only written for the
/// pair that needs it. `PIN_ALTERNATE` is a plain read/write register, same as
/// the UART remap bit, and does not need the safe-access window.
fn arm(pin: &AnyPin, line: &InterruptLine, trigger: InterruptTrigger) {
    let (edge, high) = match trigger {
        InterruptTrigger::LowLevel => (false, false),
        InterruptTrigger::HighLevel => (false, true),
        InterruptTrigger::FallingEdge => (true, false),
        InterruptTrigger::RisingEdge => (true, true),
    };
    let bit = line.bit as usize;
    let gpio = pac::GPIO;

    critical_section::with(|_| {
        if matches!(line.port, Port::B) && matches!(line.bit, 8 | 9) {
            pac::SYS.pin_alternate().modify(|w| w.set_pin_intx(line.remap));
        }
        pin.set_interrupt_polarity(high);
        match line.port {
            Port::A => {
                gpio.pa_int_mode().modify(|w| set_bit16(w, bit, edge));
                gpio.pa_int_if().write_value(pac::gpio::regs::PaIntIf(1 << bit));
                gpio.pa_int_en().modify(|w| set_bit16(w, bit, true));
            }
            Port::B => {
                gpio.pb_int_mode().modify(|w| set_bit16(w, bit, edge));
                gpio.pb_int_if().write_value(pac::gpio::regs::PbIntIf(1 << bit));
                gpio.pb_int_en().modify(|w| set_bit16(w, bit, true));
            }
        }
    });
}

fn disarm(line: &InterruptLine) {
    let bit = line.bit as usize;
    let gpio = pac::GPIO;
    critical_section::with(|_| match line.port {
        Port::A => gpio.pa_int_en().modify(|w| set_bit16(w, bit, false)),
        Port::B => gpio.pb_int_en().modify(|w| set_bit16(w, bit, false)),
    });
}

/// Set or clear one bit of a 16-bit interrupt register.
///
/// The PAC exposes these registers as one field covering the whole halfword, so
/// there is no per-pin setter to call.
fn set_bit16<T: BitHalf>(reg: &mut T, bit: usize, value: bool) {
    let mut raw = reg.half();
    if value {
        raw |= 1 << bit;
    } else {
        raw &= !(1 << bit);
    }
    reg.set_half(raw);
}

/// A 16-bit PAC register value that stores its bits in `.0`.
trait BitHalf {
    fn half(&self) -> u16;
    fn set_half(&mut self, value: u16);
}

impl BitHalf for pac::gpio::regs::PaIntEn {
    fn half(&self) -> u16 {
        self.0
    }
    fn set_half(&mut self, value: u16) {
        self.0 = value;
    }
}
impl BitHalf for pac::gpio::regs::PbIntEn {
    fn half(&self) -> u16 {
        self.0
    }
    fn set_half(&mut self, value: u16) {
        self.0 = value;
    }
}
impl BitHalf for pac::gpio::regs::PaIntMode {
    fn half(&self) -> u16 {
        self.0
    }
    fn set_half(&mut self, value: u16) {
        self.0 = value;
    }
}
impl BitHalf for pac::gpio::regs::PbIntMode {
    fn half(&self) -> u16 {
        self.0
    }
    fn set_half(&mut self, value: u16) {
        self.0 = value;
    }
}

#[cfg(feature = "embassy")]
fn on_port(port: Port) {
    let gpio = pac::GPIO;
    let (enabled, flags) = match port {
        Port::A => (gpio.pa_int_en().read().0, gpio.pa_int_if().read().0),
        Port::B => (gpio.pb_int_en().read().0, gpio.pb_int_if().read().0),
    };
    // Only pins this driver armed. A flag left set by something else must not
    // turn its enable bit back off.
    let fired = enabled & flags;
    if fired == 0 {
        return;
    }
    match port {
        Port::A => gpio.pa_int_en().modify(|w| w.0 &= !fired),
        Port::B => gpio.pb_int_en().modify(|w| w.0 &= !fired),
    }

    let wakers = &PORT_WAKERS[port as usize];
    let mut pending = fired;
    while pending != 0 {
        let bit = pending.trailing_zeros() as usize;
        pending &= !(1 << bit);
        wakers.wakers[bit].wake();
    }
}

/// One armed pin. Dropping it masks the pin again, so a cancelled wait cannot
/// leave the port interrupt asserting.
#[cfg(feature = "embassy")]
struct PinWait<'a> {
    line: InterruptLine,
    _pin: PhantomData<&'a mut AnyPin>,
}

#[cfg(feature = "embassy")]
impl<'a> PinWait<'a> {
    fn new(pin: &'a AnyPin, trigger: InterruptTrigger) -> Self {
        let line = interrupt_line(pin.port, pin.pin).unwrap_or_else(|| {
            panic!(
                "gpio: P{}{} has no port-interrupt source",
                port_letter(pin.port),
                pin.pin
            )
        });
        arm(pin, &line, trigger);
        Self {
            line,
            _pin: PhantomData,
        }
    }
}

#[cfg(feature = "embassy")]
impl Drop for PinWait<'_> {
    fn drop(&mut self) {
        disarm(&self.line);
    }
}

#[cfg(feature = "embassy")]
impl Future for PinWait<'_> {
    type Output = ();

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> Poll<()> {
        let line = &self.line;
        PORT_WAKERS[line.port as usize].wakers[line.bit as usize].register(cx.waker());

        // The handler masks the pin and leaves the flag set. A flag that is set
        // while the pin is masked belongs to this wait.
        let (enabled, flags) = match line.port {
            Port::A => (pac::GPIO.pa_int_en().read().0, pac::GPIO.pa_int_if().read().0),
            Port::B => (pac::GPIO.pb_int_en().read().0, pac::GPIO.pb_int_if().read().0),
        };
        let mask = 1u16 << line.bit;
        if flags & mask != 0 && enabled & mask == 0 {
            // Acknowledge before returning. A level that is still present sets
            // the flag again, but the pin stays masked until the next wait.
            match line.port {
                Port::A => pac::GPIO.pa_int_if().write_value(pac::gpio::regs::PaIntIf(mask)),
                Port::B => pac::GPIO.pb_int_if().write_value(pac::gpio::regs::PbIntIf(mask)),
            }
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

fn port_letter(port: Port) -> char {
    match port {
        Port::A => 'A',
        Port::B => 'B',
    }
}

// --- embedded-hal impls ---------------------------------------------------

#[cfg(feature = "embassy")]
impl<'d> embedded_hal_async::digital::Wait for Input<'d> {
    /// Wait until the pin reads high. See [`Input::wait_for_high`].
    ///
    /// # Panics
    ///
    /// Panics if the pin has no port-interrupt source.
    async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
        self.wait_for_high().await;
        Ok(())
    }

    /// Wait until the pin reads low. See [`Input::wait_for_low`].
    async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
        self.wait_for_low().await;
        Ok(())
    }

    /// Wait for the next rising edge. See [`Input::wait_for_rising_edge`].
    async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
        self.wait_for_rising_edge().await;
        Ok(())
    }

    /// Wait for the next falling edge. See [`Input::wait_for_falling_edge`].
    async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
        self.wait_for_falling_edge().await;
        Ok(())
    }

    /// Not supported: one pin stores one polarity, so it cannot watch both edges.
    ///
    /// # Panics
    ///
    /// Always panics. Wait for one edge, then arm the other.
    async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
        panic!("gpio: a pin watches one edge at a time; wait for rising and falling separately");
    }
}

impl<'d> embedded_hal::digital::ErrorType for Input<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::InputPin for Input<'d> {
    #[inline]
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    #[inline]
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for Output<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::OutputPin for Output<'d> {
    #[inline]
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_high())
    }

    #[inline]
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_low())
    }
}

impl<'d> embedded_hal::digital::StatefulOutputPin for Output<'d> {
    #[inline]
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_high())
    }

    #[inline]
    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for Flex<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::InputPin for Flex<'d> {
    #[inline]
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    #[inline]
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::OutputPin for Flex<'d> {
    #[inline]
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_high())
    }

    #[inline]
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_low())
    }
}

impl<'d> embedded_hal::digital::StatefulOutputPin for Flex<'d> {
    #[inline]
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_high())
    }

    #[inline]
    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_low())
    }
}
