//! CH582 USB CDC echo. Build with `--features usb-cdc`.
//! Connect the host to the board's USB controller pins, not the debug probe.
//! The driver is experimental; EP0 DMA can race a new SETUP packet.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use hal::peripherals::USB as Controller;
use hal::usb::InterruptHandler;

hal::bind_interrupts!(struct Irqs {
    USB => InterruptHandler<Controller>;
});

fn controller(p: hal::Peripherals) -> hal::Peri<'static, Controller> {
    p.USB
}

#[path = "../usb_cdc.rs"]
mod usb_cdc;
