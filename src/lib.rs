//! HAL for WCH's CH5xx BLE microcontrollers.
//!
//! Ported from `ch32-hal`, adjusted for the CH5xx register model:
//!
//! - **No RCC.** Clocks, power and the pin mux live in `SYS` /
//!   `System_control` at `0x4000_1000`. [`rcc`] configures the clock tree and
//!   caches the resulting frequency.
//! - **No AFIO.** Peripheral signals are selected with the `SYS.PIN_ALTERNATE`
//!   "alternate pin enable" bits.
//! - **One GPIO block**, `pac::GPIO`, with per-port register groups
//!   (`pa_out`, `pb_dir`, …) instead of one peripheral per port.
//!
//! [`rcc`] is a plain module rather than a driver directory because the tree is
//! a single path with no peripheral prescalers: there is one frequency to
//! report.

#![no_std]
#![allow(static_mut_refs)]
#![allow(unexpected_cfgs)]

// The QingKe V3C core implements RV32IMBC: no A extension. `amoadd.w` and
// `amoswap.w` trap on real CH585 silicon (measured; see the `embassy` module
// docs), so the standard target's atomics cannot be used. Every atomic in this
// crate goes through `portable-atomic`, which falls back to a critical
// section. Plain loads/stores still compile to `lw`/`sw`.
extern crate portable_atomic as atomic;

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

    /// Async mode, driven by the peripheral interrupt.
    #[cfg(feature = "embassy")]
    pub struct Async;

    #[cfg(feature = "embassy")]
    impl_mode!(Async);
}

pub mod rcc;

pub mod debug;
pub mod prelude;

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

#[cfg(adc_ch585)]
pub mod adc;
pub mod delay;
#[cfg(tmr_all_dma)]
pub mod dma;
pub mod gpio;
pub mod i2c;
#[cfg(pwmx_ch585)]
pub mod pwm;
#[cfg(power_sleep)]
pub mod power;
#[cfg(rtc_calendar)]
pub mod rtc;
#[cfg(spi_ch585)]
pub mod spi;
#[cfg(tmr_all_dma)]
pub mod timer;
pub mod uart;
#[cfg(all(feature = "embassy", usbfs_ch585))]
pub mod usb;
#[cfg(all(feature = "embassy", usbhs_ch585))]
pub mod usbhs;
pub mod watchdog;

/// Embassy integration (time driver). Requires the `embassy` feature.
#[cfg(feature = "embassy")]
pub mod embassy;

pub struct Config {
    /// System clock tree applied before any driver starts.
    pub rcc: rcc::Config,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rcc: Default::default(),
        }
    }
}

impl From<rcc::Config> for Config {
    /// Boot with this clock tree and the default for everything else.
    fn from(rcc: rcc::Config) -> Self {
        Self { rcc }
    }
}

/// Initialize the HAL and return the peripheral singletons.
///
/// Panics if called twice.
pub fn init(config: Config) -> Peripherals {
    // First, so a second call panics before anything is touched.
    let p = Peripherals::take();

    unsafe {
        // Apply the clock tree first: SysTick counts Fsys, so the calibration
        // and the time driver below must see the final clock.
        rcc::set_config(config.rcc);
        // `delay` measures the real SysTick rate against the RTC and keeps it
        // for its busy-waits. `rcc::clocks()` keeps reporting the nominal
        // frequency the tree was configured to.
        delay::init();
        // The time driver starts its counter and enables its own interrupt. It
        // reads the frequency from `rcc::clocks()`, so it must run after the
        // calibration above. It also enables global interrupts: qingke-rt's V3C
        // startup leaves them clear, so after `init()` interrupts work.
        #[cfg(feature = "embassy")]
        embassy::time_driver_impl::init();
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
