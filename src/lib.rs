//! HAL for WCH's CH5xx BLE microcontrollers.
//!
//! Ported from `ch32-hal`, adjusted for the CH5xx register model:
//!
//! - **No RCC.** Clocks, power and the pin mux live in `SYS` /
//!   `System_control` at `0x4000_1000`. [`rcc`] only caches the resulting
//!   frequencies.
//! - **No AFIO.** Peripheral signals are selected with the `SYS.PIN_ALTERNATE`
//!   "alternate pin enable" bits.
//! - **One GPIO block**, `pac::GPIO`, with per-port register groups
//!   (`pa_out`, `pb_dir`, …) instead of one peripheral per port.
//!
//! [`rcc`] is a plain module rather than a driver directory: with no PLL setup
//! implemented yet there is one frequency to report.

#![no_std]
#![allow(static_mut_refs)]
#![allow(unexpected_cfgs)]

pub use ch5xx_metapac as pac;

pub(crate) use embassy_hal_internal::{impl_peripheral, peripherals_definition, peripherals_struct};
pub use embassy_hal_internal::{Peri, PeripheralType};

#[cfg(feature = "rt")]
pub use qingke_rt::{entry, interrupt};

// Must come first: later modules use these macros.
include!(concat!(env!("OUT_DIR"), "/_macros.rs"));

pub(crate) mod internal;

mod macros;

pub mod time;

/// Operating modes for peripherals.
pub mod mode {
    trait SealedMode {}

    /// Operating mode for a peripheral.
    #[allow(private_bounds)]
    pub trait Mode: SealedMode {}

    macro_rules! impl_mode {
        ($name:ident) => {
            impl SealedMode for $name {}
            impl Mode for $name {}
        };
    }

    /// Blocking mode.
    pub struct Blocking;

    impl_mode!(Blocking);
}

pub mod rcc;

pub mod debug;
pub mod prelude;

mod peripheral;
pub use peripheral::Peripheral;

mod interrupt_ext;

// The `interrupt` module, generated because the interrupt list is per-chip.
include!(concat!(env!("OUT_DIR"), "/_interrupts.rs"));

// Must come last, so the `impl_foo!` macros above are all in scope.
pub(crate) mod _generated {
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(non_snake_case)]
    #![allow(missing_docs)]

    include!(concat!(env!("OUT_DIR"), "/_generated.rs"));
}

pub use crate::_generated::{peripherals, Peripherals};

pub mod delay;
pub mod gpio;

pub struct Config {
    pub rcc: rcc::Config,
}

impl Default for Config {
    fn default() -> Self {
        Self { rcc: Default::default() }
    }
}

/// Initialize the HAL and return the peripheral singletons.
///
/// Panics if called twice.
pub fn init(config: Config) -> Peripherals {
    // First, so a second call panics before anything is touched.
    let p = Peripherals::take();

    unsafe {
        rcc::init(config.rcc);
        delay::init();
    }

    critical_section::with(|cs| unsafe {
        gpio::init(cs);
    });

    p
}

#[macro_export]
macro_rules! bind_interrupts {
    ($vis:vis struct $name:ident { $($irq:ident => $($handler:ty),*;)* }) => {
        #[derive(Copy, Clone)]
        $vis struct $name;

        $(
            #[allow(non_snake_case)]
            #[$crate::interrupt]
            unsafe fn $irq() {
                $(
                    <$handler as $crate::interrupt::typelevel::Handler<$crate::interrupt::typelevel::$irq>>::on_interrupt();
                )*
            }

            $(
                unsafe impl $crate::interrupt::typelevel::Binding<$crate::interrupt::typelevel::$irq, $handler> for $name {}
            )*
        )*
    };
}
