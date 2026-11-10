//! CH585 timers: one counter/FIFO per instance, not CH32 multi-channel TIMs.
//!
//! Normal counting runs at Fsys without a prescaler. TMR0/3 additionally have
//! the SDK's NFC clock mode. DMA belongs to the timer, not a separate channel.
//! On dies where TMR0/TMR3 use the `TMR_NODMA` block (CH592, CH595) those
//! instances have no DMA registers, so the transfer APIs are bounded on
//! [`DmaInstance`] and the counter, FIFO and interrupt paths remain available.
//! Clock changes and deep sleep require stopping and rebuilding active drivers.

use crate::{interrupt, pac, peripherals};

pub mod counter;
pub mod input_capture;
pub mod low_level;
pub mod simple_pwm;
mod types;
pub use types::*;

#[allow(private_bounds)]
pub trait Instance: crate::PeripheralType + SealedInstance + Send + 'static {
    type Interrupt: interrupt::typelevel::Interrupt;
}

/// Timer instances with the timer-local DMA engine.
///
/// Some dies omit the DMA registers on TMR0/TMR3 (`TMR_NODMA`); the counter,
/// capture, PWM and interrupt APIs work there, but no transfer can start.
pub trait DmaInstance: Instance {}

/// Timer instances supporting the SDK's dedicated NFC clock.
pub trait NfcClockInstance: Instance {}

pub(crate) trait SealedInstance {
    /// The full (DMA-capable) register block.
    ///
    /// A no-DMA instance shares every offset this driver uses except the
    /// timer-local DMA registers, which the DMA entry points never touch
    /// because they require [`DmaInstance`]. The pointer cast keeps one
    /// implementation for both block types instead of duplicating the driver.
    fn regs() -> pac::tmr::TmrFull;
    /// Whether the instance has the timer-local DMA engine.
    const HAS_DMA: bool;
    unsafe fn set_remap(remap: bool);
    #[cfg(feature = "embassy")]
    fn state() -> &'static State;
}

#[cfg(feature = "embassy")]
pub(crate) struct State {
    pub events: crate::atomic::AtomicU8,
    pub rx_finite: crate::atomic::AtomicBool,
    pub waker: embassy_sync::waitqueue::AtomicWaker,
}

macro_rules! instance {
    ($name:ident, $set:ident, $nodma:ident, $remapcfg:ident) => {
        impl SealedInstance for peripherals::$name {
            fn regs() -> pac::tmr::TmrFull {
                unsafe { pac::tmr::TmrFull::from_ptr(pac::$name.as_ptr()) }
            }
            const HAS_DMA: bool = !cfg!($nodma);
            unsafe fn set_remap(remap: bool) {
                #[cfg($remapcfg)]
                pac::SYS.pin_alternate().modify(|w| w.$set(remap));
                // Dies without this remap field route the timer to a fixed pin.
                #[cfg(not($remapcfg))]
                let _ = remap;
            }
            #[cfg(feature = "embassy")]
            fn state() -> &'static State {
                static STATE: State = State {
                    events: crate::atomic::AtomicU8::new(0),
                    rx_finite: crate::atomic::AtomicBool::new(false),
                    waker: embassy_sync::waitqueue::AtomicWaker::new(),
                };
                &STATE
            }
        }
        impl Instance for peripherals::$name {
            type Interrupt = interrupt::typelevel::$name;
        }
        #[cfg(not($nodma))]
        impl DmaInstance for peripherals::$name {}
    };
}
#[cfg(not(feature = "time-driver-tmr0"))]
instance!(TMR0, set_pin_tmr0, tmr0_nodma, remap_tmr0);
// CH572/CH570 have one timer only.
#[cfg(all(tmr1, not(feature = "time-driver-tmr1")))]
instance!(TMR1, set_pin_tmr1, tmr1_nodma, remap_tmr1);
#[cfg(all(tmr2, not(feature = "time-driver-tmr2")))]
instance!(TMR2, set_pin_tmr2, tmr2_nodma, remap_tmr2);
#[cfg(all(tmr3, not(feature = "time-driver-tmr3")))]
instance!(TMR3, set_pin_tmr3, tmr3_nodma, remap_tmr3);
// The NFC clock mode exists only on dies with an NFC front end.
#[cfg(all(nfc_clock, not(feature = "time-driver-tmr0")))]
impl NfcClockInstance for peripherals::TMR0 {}
#[cfg(all(nfc_clock, tmr3, not(feature = "time-driver-tmr3")))]
impl NfcClockInstance for peripherals::TMR3 {}

mod sealed {
    pub trait Pin {}
}
/// External TMR input/output pin with a checked remap selection.
#[allow(private_bounds)]
pub trait TimerPin<T: Instance, const REMAP: u8>: crate::gpio::Pin + sealed::Pin {}
// Unused on a die whose timer pins the family file does not route.
#[allow(unused_macros)]
macro_rules! pin {
    ($tmr:ident, $remap:literal, $pin:ident) => {
        impl sealed::Pin for peripherals::$pin {}
        impl TimerPin<peripherals::$tmr, $remap> for peripherals::$pin {}
    };
}
#[cfg(all(not(feature = "time-driver-tmr0"), remap_tmr0))]
pin!(TMR0, 0, PA9);
#[cfg(all(not(feature = "time-driver-tmr0"), remap_tmr0))]
pin!(TMR0, 1, PB23);
#[cfg(all(tmr1, not(feature = "time-driver-tmr1"), remap_tmr1))]
pin!(TMR1, 0, PA10);
#[cfg(all(tmr1, not(feature = "time-driver-tmr1"), remap_tmr1))]
pin!(TMR1, 1, PB10);
#[cfg(all(tmr2, not(feature = "time-driver-tmr2"), remap_tmr2))]
pin!(TMR2, 0, PA11);
#[cfg(all(tmr2, not(feature = "time-driver-tmr2"), remap_tmr2))]
pin!(TMR2, 1, PB11);
#[cfg(all(tmr3, not(feature = "time-driver-tmr3"), remap_tmr3))]
pin!(TMR3, 0, PB22);
#[cfg(all(tmr3, not(feature = "time-driver-tmr3"), remap_tmr3))]
pin!(TMR3, 1, PA2);

/// One handler for the instance's currently enabled timer events.
///
/// Masks reported sources rather than repeatedly clearing a level event. In
/// particular DMA_END must not be acknowledged as an ordinary W1C event while
/// NOW equals END in non-loop mode. No borrowed pointers are stored in this ISR.
#[cfg(feature = "embassy")]
pub struct InterruptHandler<T: Instance>(core::marker::PhantomData<T>);

#[cfg(feature = "embassy")]
impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let r = T::regs();
        let events = r.int_flag().read().0 & r.inter_en().read().0;
        r.inter_en().modify(|w| w.0 &= !events);
        if T::HAS_DMA
            && events & Events::DMA_END.bits() != 0
            && T::state().rx_finite.load(crate::atomic::Ordering::Relaxed)
        {
            // Do not let an exhausted finite capture fill/overflow the FIFO
            // while its task is waiting to resume. TX must instead drain.
            r.ctrl_mod().modify(|w| w.set_count_en(false));
            r.ctrl_dma().write(|w| w.0 = 0);
            crate::dma::fence();
        }
        T::state().events.fetch_or(events, crate::atomic::Ordering::Relaxed);
        T::state().waker.wake();
    }
}

impl embedded_hal::pwm::Error for Error {
    fn kind(&self) -> embedded_hal::pwm::ErrorKind {
        embedded_hal::pwm::ErrorKind::Other
    }
}
