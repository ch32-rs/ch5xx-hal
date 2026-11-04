//! Exercise UART0 in the peripheral's internal loopback mode.
//!
//! `MCR.LOOP` connects TXD back to RXD inside the chip, so this verifies the
//! baud dividers, FIFO setup and both directions without any external wiring.
//! The default UART0 pins are PB4 (RXD) and PB7 (TXD).
//!
//! 115200 is fine here even though this board's clock realises it as ~111.1
//! kbaud: both ends of the loop share the same divider, so the error cancels.
//! Talking to an external device is what needs `Config::new(9600)`.
//!
//! The receiver has an eight-byte FIFO and nothing drains it while
//! `blocking_flush` waits for transmission, so the payload is sent and read in
//! chunks no larger than the FIFO. Sending more than eight bytes before reading
//! would overrun and make `blocking_read` wait forever.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::uart::{Config, Uart};
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    // 115200 8N1 is the default.
    let config = Config::default();

    let mut uart = Uart::new_blocking::<0>(p.UART0, p.PB4, p.PB7, config).unwrap();

    // MCR.LOOP connects TXD to RXD internally and makes all external UART
    // outputs inactive. It must be driven by polling: on CH585 the UART
    // interrupt output is not asserted while loopback is enabled.
    uart.set_internal_loopback(true);

    let sent = b"CH585 UART loopback";
    let mut received = [0u8; 19];
    let mut ok = true;

    for (out, inp) in sent.chunks(8).zip(received.chunks_mut(8)) {
        uart.blocking_write(out).unwrap();
        uart.blocking_flush().unwrap();
        if uart.blocking_read(inp).is_err() {
            ok = false;
            break;
        }
        if out != inp {
            ok = false;
        }
    }

    if ok && &received == sent {
        defmt::info!("uart loopback ok, {} bytes", sent.len());
    } else {
        defmt::error!("uart loopback mismatch");
    }

    loop {
        core::hint::spin_loop();
    }
}
