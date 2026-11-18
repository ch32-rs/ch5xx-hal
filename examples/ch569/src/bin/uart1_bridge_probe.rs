//! UART1 host bridge on J1: echo received bytes, heartbeat when idle.

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
    let mut delay = hal::delay::Delay;
    let uart = Uart::new_blocking(p.UART1, p.PA7, p.PA8, Config::new(115_200)).unwrap();
    let (mut tx, mut rx) = uart.split();
    let regs = hal::pac::UART1;

    scratch(0, u32::from_le_bytes(*b"U1BR"));
    scratch(1, 0);
    scratch(2, 0);
    scratch(3, 0);

    // Three banners catch a host that opens the port after the first one.
    for _ in 0..3 {
        tx.blocking_write(b"\r\nCH569 UART1 READY 115200\r\n").unwrap();
        delay.delay_ms(50);
    }

    let mut total = 0u32;
    let mut errors = 0u32;
    let mut last = 0u32;
    let mut idle = 0u32;
    loop {
        if regs.rfc().read().rfc() != 0 {
            let mut buf = [0u8; 16];
            match rx.blocking_read_some(&mut buf) {
                Ok(n) => {
                    tx.blocking_write(&buf[..n]).unwrap();
                    total = total.wrapping_add(n as u32);
                    let mut tail = [0u8; 4];
                    let take = n.min(4);
                    tail[..take].copy_from_slice(&buf[n - take..n]);
                    last = u32::from_le_bytes(tail);
                }
                Err(_) => errors = errors.wrapping_add(1),
            }
            scratch(1, total);
            scratch(2, errors);
            scratch(3, last);
            idle = 0;
        } else {
            // Poll often: the receive FIFO is eight bytes deep (87 us/byte at
            // 115200), so a 100 ms nap loses the tail of a host burst.
            delay.delay_us(100);
            idle += 1;
            // One 'H' every ~500 ms proves the transmit path while idle.
            if idle % 5000 == 0 {
                let _ = tx.blocking_write(b"H");
            }
        }
    }
}
