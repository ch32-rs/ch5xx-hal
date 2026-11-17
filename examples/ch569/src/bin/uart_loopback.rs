//! UART0 internal loopback: `MCR.LOOP` wires TXD to RXD inside the block.
//!
//! UART1-3 are the no-modem block on CH569 and have no loopback bit, so this
//! uses UART0 on its default pins PB6 (TXD) / PB5 (RXD). Nothing needs to be
//! connected: the bytes go out of the shift register and straight back in.
//!
//! Results go to RAMX as well as RTT: an RTT attach halts the core between
//! reads, so the iteration count and error count are read back instead.
//!
//! ```console
//! cargo build --release --bin uart_loopback
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 5
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::uart::{Config, Uart};
use defmt::info;
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    unsafe {
        core::ptr::write_volatile(SCRATCH, 0x5552_4154); // "TARU"
        core::ptr::write_volatile(SCRATCH.add(1), 0);
        core::ptr::write_volatile(SCRATCH.add(2), 0);
    }
    info!("Fsys = {} Hz", hal::rcc::clocks().sysclk.0);
    let mut uart = Uart::new_blocking::<0>(p.UART0, p.PB5, p.PB6, Config::default()).unwrap();
    uart.set_internal_loopback(true);
    info!("loopback on");

    let mut n: u8 = 0;
    loop {
        let pattern = [n, n.wrapping_add(1), n.wrapping_add(2), 0xA5];
        uart.blocking_write(&pattern).unwrap();
        uart.blocking_flush().unwrap();

        let mut rx = [0u8; 4];
        let (ok, stored) = match uart.blocking_read(&mut rx) {
            Ok(()) => (rx == pattern, u32::from_be_bytes(rx)),
            Err(_) => (false, 0xdead_0000),
        };
        unsafe {
            core::ptr::write_volatile(SCRATCH.add(1), n as u32 + 1);
            core::ptr::write_volatile(SCRATCH.add(2), stored);
            if !ok {
                core::ptr::write_volatile(SCRATCH.add(3), stored | 0x1_0000);
            }
        }
        match ok {
            true => info!("{}: {=[u8]:#04x} ok", n, rx),
            false => info!("{}: sent {=[u8]:#04x}, got {=[u8]:#04x}", n, pattern, rx),
        }
        n = n.wrapping_add(1);
        hal::delay::Delay.delay_ms(50);
    }
}
