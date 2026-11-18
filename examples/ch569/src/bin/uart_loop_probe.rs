//! Register-level UART transmit self-test on CH569.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::uart::{self, Config, RxPin, TxPin, Uart};
use ch5xx_hal::Peri;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const PATTERN: [u8; 8] = *b"CH569LP!";
const POLL_LIMIT: u32 = 1_000_000;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

/// UART0 only: send `PATTERN` through `MCR.LOOP` and compare what comes back.
fn loopback_check(
    peri: Peri<'_, hal::peripherals::UART0>,
    rx: Peri<'_, hal::peripherals::PB5>,
    tx: Peri<'_, hal::peripherals::PB6>,
    slot: usize,
    regs: hal::pac::uart::UartFull,
) -> u32 {
    let mut uart = Uart::new_blocking(peri, rx, tx, Config::new(115_200)).unwrap();
    uart.set_internal_loopback(true);
    let (mut tx, mut rx) = uart.split();
    tx.blocking_write(&PATTERN).unwrap();
    tx.blocking_flush().unwrap();

    let mut got = [0u8; 8];
    let mut n = 0usize;
    let mut err = 0u32;
    for _ in 0..POLL_LIMIT {
        if n == got.len() {
            break;
        }
        if regs.rfc().read().rfc() != 0 {
            match rx.blocking_read_some(&mut got[n..]) {
                Ok(k) => n += k,
                Err(_) => {
                    err += 1;
                    break;
                }
            }
        }
    }

    let word = |i: usize| u32::from_le_bytes([got[i], got[i + 1], got[i + 2], got[i + 3]]);
    scratch(slot * 4, n as u32 | err << 16);
    scratch(slot * 4 + 1, word(0));
    scratch(slot * 4 + 2, word(4));
    let ok = (n == got.len() && got == PATTERN) as u32;
    scratch(slot * 4 + 3, ok);
    ok
}

/// UART1/3: the block has no loopback, so check that a line really leaves.
fn tx_check<T, R, X>(
    peri: Peri<'_, T>,
    rx: Peri<'_, R>,
    tx: Peri<'_, X>,
    slot: usize,
    regs: hal::pac::uart::UartNomodem,
) -> u32
where
    T: uart::Instance,
    R: RxPin<T, 0>,
    X: TxPin<T, 0>,
{
    let uart = Uart::new_blocking(peri, rx, tx, Config::new(115_200)).unwrap();
    let (mut tx, _rx) = uart.split();
    tx.blocking_write(b"CH569 UARTn TX OK\r\n").unwrap();

    // `blocking_write` only waits for FIFO room, so poll until the shift
    // register is empty too.
    let mut drained = false;
    for _ in 0..POLL_LIMIT {
        if regs.lsr().read().lsr_tx_all_emp() {
            drained = true;
            break;
        }
    }
    let fifo_empty = regs.tfc().read().tfc() == 0;

    scratch(slot * 4, drained as u32);
    scratch(slot * 4 + 1, fifo_empty as u32);
    scratch(slot * 4 + 2, 0);
    let ok = (drained && fifo_empty) as u32;
    scratch(slot * 4 + 3, ok);
    ok
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    scratch(0, u32::from_le_bytes(*b"ULOP"));
    scratch(1, 0);
    scratch(2, 0);
    scratch(3, 0);

    let mut ok = 0;
    ok += loopback_check(p.UART0, p.PB5, p.PB6, 1, hal::pac::UART0);
    ok += tx_check(p.UART1, p.PA7, p.PA8, 2, hal::pac::UART1);
    ok += tx_check(p.UART3, p.PB3, p.PB4, 3, hal::pac::UART3);

    scratch(1, ok);
    scratch(2, if ok == 3 { 0x5041_5353 } else { 0x4641_494c }); // "PASS" / "FAIL"

    loop {
        core::hint::spin_loop();
    }
}
