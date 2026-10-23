//! Peripheral marker traits.
//!
//! ch32-hal's `RccPeripheral` / `RemapPeripheral` have no CH5xx counterpart:
//! there is no RCC to gate a peripheral's clock with (the vendor enables
//! peripheral clocks through `SYS` power/sleep bits, not a per-peripheral
//! enable bit) and no AFIO to program a remap group with. What is left is the
//! bound drivers put on their peripheral parameter.

use embassy_hal_internal::PeripheralType;

/// A peripheral this HAL can hand out.
pub trait Peripheral: PeripheralType + 'static {}
