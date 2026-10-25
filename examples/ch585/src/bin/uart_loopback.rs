//! Exercise UART0 in the peripheral's internal loopback mode.
//!
//! `MCR.LOOP` connects TXD back to RXD inside the chip, so this verifies the
//! baud dividers, FIFO setup and both directions without any external wiring.
//! The default UART0 pins are PB4 (RXD) and PB7 (TXD).
//!
//! The receiver has an eight-byte FIFO and nothing drains it while
//! `blocking_flush` waits for transmission, so the payload is sent and read in
//! chunks no larger than the FIFO. Sending more than eight bytes before reading
//! would overrun and make `blocking_read` wait forever.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::pac;
use hal::uart::{Config, Uart};
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut config = Config::default();
    config.baudrate = 9600;

    let mut uart = Uart::new_blocking::<0>(p.UART0, p.PB4, p.PB7, config).unwrap();

    // Datasheet chapter 9: MCR.LOOP connects TXD to RXD internally and makes all
    // external UART outputs inactive.
    pac::UART0.mcr().modify(|w| w.set_mcr_loop(true));

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
