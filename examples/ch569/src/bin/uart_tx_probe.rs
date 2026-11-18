//! Send a line on every UART pin group and let the host say which one arrived.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::uart::{Config, Uart};
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    scratch(0, u32::from_le_bytes(*b"UART"));
    scratch(1, 0);
    scratch(2, 0);
    scratch(3, 0);

    let mut u0 = Uart::new_blocking(p.UART0, p.PB5, p.PB6, Config::new(115_200)).unwrap();
    let mut u1 = Uart::new_blocking(p.UART1, p.PA7, p.PA8, Config::new(115_200)).unwrap();
    let mut u3 = Uart::new_blocking(p.UART3, p.PB3, p.PB4, Config::new(115_200)).unwrap();

    let mut delay = hal::delay::Delay;
    let mut n = 0u32;
    loop {
        u0.blocking_write(b"CH569 UART0 TXD0 PB6\r\n").unwrap();
        u1.blocking_write(b"CH569 UART1 TXD1 PA8\r\n").unwrap();
        u3.blocking_write(b"CH569 UART3 TXD3 PB4\r\n").unwrap();
        n = n.wrapping_add(1);
        scratch(1, n);
        delay.delay_ms(100);
    }
}
