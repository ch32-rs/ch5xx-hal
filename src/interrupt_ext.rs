//! Interrupt plumbing: the `interrupt` module shape plus `InterruptExt`.
//!
//! Same shape as ch32-hal's. The PFIC accessors come from `qingke`, which takes
//! the same path on the V3C core as on V3A/V3B (CSR `0x800` is remapped to
//! `mstatus.MIE`/`MPIE` once the runtime sets `corecfgr.IE_REMAP_EN`).

use core::sync::atomic::{compiler_fence, Ordering};

use critical_section::CriticalSection;
pub use qingke::interrupt::Priority;
use qingke::pfic;

use crate::pac::InterruptNumber;

/// Generate a standard `mod interrupt` for a HAL.
#[macro_export]
macro_rules! interrupt_mod {
    ($($irqs:ident),* $(,)?) => {
        /// Interrupt definitions.
        pub mod interrupt {
            pub use crate::interrupt_ext::{InterruptExt, Priority};
            pub use crate::pac::Interrupt;
            pub use crate::pac::Interrupt::*;

            /// Type-level interrupt infrastructure.
            ///
            /// One *type* per interrupt, used to check at compile time that
            /// drivers have their interrupts bound.
            pub mod typelevel {
                use super::InterruptExt;

                trait SealedInterrupt {}

                /// Type-level interrupt.
                #[allow(private_bounds)]
                pub trait Interrupt: SealedInterrupt {
                    /// Interrupt enum variant.
                    const IRQ: super::Interrupt;

                    /// Enable the interrupt.
                    #[inline]
                    unsafe fn enable() {
                        Self::IRQ.enable()
                    }

                    /// Disable the interrupt.
                    #[inline]
                    fn disable() {
                        Self::IRQ.disable()
                    }

                    /// Check if the interrupt is enabled.
                    #[inline]
                    fn is_enabled() -> bool {
                        Self::IRQ.is_enabled()
                    }

                    /// Check if the interrupt is pending.
                    #[inline]
                    fn is_pending() -> bool {
                        Self::IRQ.is_pending()
                    }

                    /// Set the interrupt pending.
                    #[inline]
                    fn pend() {
                        Self::IRQ.pend()
                    }

                    /// Unset the interrupt pending.
                    #[inline]
                    fn unpend() {
                        Self::IRQ.unpend()
                    }

                    /// Get the priority of the interrupt.
                    #[inline]
                    fn get_priority() -> crate::interrupt::Priority {
                        Self::IRQ.get_priority()
                    }

                    /// Set the interrupt priority.
                    #[inline]
                    fn set_priority(prio: crate::interrupt::Priority) {
                        Self::IRQ.set_priority(prio)
                    }

                    /// Set the interrupt priority with an already-acquired critical section.
                    #[inline]
                    fn set_priority_with_cs(cs: critical_section::CriticalSection, prio: crate::interrupt::Priority) {
                        Self::IRQ.set_priority_with_cs(cs, prio)
                    }
                }

                $(
                    #[allow(non_camel_case_types)]
                    #[doc = concat!(stringify!($irqs), " typelevel interrupt.")]
                    pub enum $irqs {}
                    impl SealedInterrupt for $irqs {}
                    impl Interrupt for $irqs {
                        const IRQ: super::Interrupt = super::Interrupt::$irqs;
                    }
                )*

                /// Interrupt handler trait.
                pub trait Handler<I: Interrupt> {
                    /// Interrupt handler body.
                    ///
                    /// # Safety
                    ///
                    /// Must only be called from the interrupt handler for `I`.
                    unsafe fn on_interrupt();
                }

                /// Compile-time assertion that an interrupt has been bound.
                ///
                /// # Safety
                ///
                /// Implementing this asserts that `H::on_interrupt()` is called
                /// every time interrupt `I` fires.
                pub unsafe trait Binding<I: Interrupt, H: Handler<I>> {}
            }
        }
    };
}

/// Interrupt operations available on every interrupt number.
pub unsafe trait InterruptExt: InterruptNumber + Copy {
    /// Enable the interrupt.
    #[inline]
    unsafe fn enable(self) {
        compiler_fence(Ordering::SeqCst);
        pfic::enable_interrupt(self.number() as u8)
    }

    /// Disable the interrupt.
    #[inline]
    fn disable(self) {
        unsafe { pfic::disable_interrupt(self.number() as u8) };
        compiler_fence(Ordering::SeqCst);
    }

    /// Check if the interrupt is being handled.
    #[inline]
    fn is_active(self) -> bool {
        pfic::is_active(self.number() as u8)
    }

    /// Check if the interrupt is enabled.
    #[inline]
    fn is_enabled(self) -> bool {
        pfic::is_enabled(self.number() as u8)
    }

    /// Check if the interrupt is pending.
    #[inline]
    fn is_pending(self) -> bool {
        pfic::is_pending(self.number() as u8)
    }

    /// Set the interrupt pending.
    #[inline]
    fn pend(self) {
        unsafe { pfic::pend_interrupt(self.number() as u8) }
    }

    /// Unset the interrupt pending.
    #[inline]
    fn unpend(self) {
        unsafe { pfic::unpend_interrupt(self.number() as u8) }
    }

    /// Get the interrupt priority.
    #[inline]
    fn get_priority(self) -> Priority {
        Priority::from(pfic::get_priority(self.number() as u8))
    }

    /// Set the interrupt priority.
    #[inline]
    fn set_priority(self, prio: Priority) {
        unsafe { pfic::set_priority(self.number() as u8, prio.into()) }
    }

    /// Set the interrupt priority with an already-acquired critical section.
    #[inline]
    fn set_priority_with_cs(self, _cs: CriticalSection, prio: Priority) {
        unsafe { pfic::set_priority(self.number() as u8, prio.into()) }
    }
}

unsafe impl<T: InterruptNumber + Copy> InterruptExt for T {}
