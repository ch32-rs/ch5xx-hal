//! CH582 USB2 CDC echo. Build with `--features usb-cdc`.
//! Connect the host to the board's USB2 controller pins, not the debug probe.
//! The driver is experimental; EP0 DMA can race a new SETUP packet.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use hal::peripherals::USB2 as Controller;
use hal::usb::InterruptHandler;

hal::bind_interrupts!(struct Irqs {
    USB2 => InterruptHandler<Controller>;
});

fn controller(p: hal::Peripherals) -> hal::Peri<'static, Controller> {
    p.USB2
}

#[path = "../usb_cdc.rs"]
mod usb_cdc;
