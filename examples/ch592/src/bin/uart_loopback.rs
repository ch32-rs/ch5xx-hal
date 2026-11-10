//! Exercise UART0 in the peripheral's internal loopback mode on CH592.
//!
//! `MCR.LOOP` connects TXD back to RXD inside the chip, so this checks the baud
//! dividers, FIFO setup and both directions without external wiring. UART0 is
//! the `UART_FULL` block on CH592; UART1-3 are `UART_NOMODEM` and have no LOOP
//! bit.
//!
//! The receiver FIFO is eight bytes and nothing drains it while
//! `blocking_flush` waits, so the payload is sent and read in chunks no larger
//! than that. The verdict is logged once as `SELFTEST PASS`/`SELFTEST FAIL`.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::uart::{Config, Uart};
use defmt::{error, info};
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    // 9600 8N1: representable from the default 6.4 MHz clock.
    let mut uart = Uart::new_blocking::<0>(p.UART0, p.PB4, p.PB7, Config::new(9600)).unwrap();
    uart.set_internal_loopback(true);

    let sent = b"CH592 UART0 loopback";
    let mut received = [0u8; 20];
    let mut ok = true;

    for (chunk, inp) in sent.chunks(8).zip(received.chunks_mut(8)) {
        uart.blocking_write(chunk).unwrap();
        uart.blocking_flush().unwrap();
        if uart.blocking_read(inp).is_err() || chunk != inp {
            ok = false;
            break;
        }
    }
    ok &= &received == sent;

    if ok {
        info!("uart0 loopback ok: {} bytes, SELFTEST PASS", sent.len());
    } else {
        error!("uart0 loopback mismatch, SELFTEST FAIL");
    }

    let mut delay = hal::delay::Delay;
    loop {
        info!(
            "heartbeat, uart0 loopback {}",
            if ok { "ok" } else { "FAILED" }
        );
        delay.delay_ms(1000);
    }
}
