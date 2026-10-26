//! Echo bytes between UART3 and an external device, using the async driver.
//!
//! UART3's default pins are PA4 (RXD) and PA5 (TXD). On the CH585M-R1-1v1
//! evaluation board they are broken out next to each other on header P2,
//! pins 1 and 2. The board sends one banner, echoes every received byte, and
//! mirrors each finished line (up to 64 bytes) to `defmt` over RTT, so the same
//! traffic can be watched from `cargo run` without opening the serial port.
//!
//! Wiring, with the device's TX and RX at 3.3 V logic:
//!
//! ```text
//! device TX  -> PA4 / P2.1 (RXD)
//! device RX  -> PA5 / P2.2 (TXD)
//! device GND -> GND / P2.24 (or 25/27)
//! ```
//!
//! This example runs **9600 8N1**, not the usual 115200 default: at the
//! measured ~5.33 MHz system clock 115200 cannot be generated. The baud is
//! `Fsys / (8 * DIV * DL)` with integer `DIV` and `DL`, so 115200 would need
//! `DIV * DL = 5.787`; the nearest whole products give 111.1 kbaud (-3.5%) or
//! 133.3 kbaud (+15.7%). -3.5% is outside what a normal UART receiver tolerates,
//! which shows up as intermittent framing errors and dropped bytes. 9600 needs
//! `DIV * DL = 69.4`, and `DL = 69` lands within +0.6%.
//!
//! `defmt` writes hold a critical section, so a line log delays the next read by
//! a few milliseconds; a sender that keeps transmitting during the log can
//! overrun the eight-byte FIFO. For interactive traffic that does not matter.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::mode::Async;
use hal::peripherals::UART3;
use hal::uart::{Config, Error, InterruptHandler, Uart};
use panic_halt as _;
use qingke_rt::entry;

hal::bind_interrupts!(struct Irqs {
    UART3 => InterruptHandler<UART3>;
});

/// Longest line mirrored to RTT. Longer input is split, not dropped.
const LINE_MAX: usize = 64;

#[embassy_executor::task]
async fn echo(mut uart: Uart<'static, UART3, Async>, baudrate: u32) -> ! {
    uart.write(b"CH585 UART3 echo\r\n").await.unwrap();
    uart.flush().await.unwrap();
    defmt::info!("uart echo ready on PA4/PA5, {} 8N1", baudrate);

    // Everything received is echoed straight back; line endings also go to
    // defmt, so the same bytes can be watched over RTT.
    let mut line = [0u8; LINE_MAX];
    let mut len = 0usize;
    let mut byte = [0u8; 1];

    loop {
        match uart.read(&mut byte).await {
            Ok(()) => {
                uart.write(&byte).await.unwrap();

                if byte[0] == b'\n' || len == line.len() {
                    // Drop the CR of a CRLF pair so the log line is clean.
                    let end = if len > 0 && line[len - 1] == b'\r' {
                        len - 1
                    } else {
                        len
                    };
                    match core::str::from_utf8(&line[..end]) {
                        Ok(text) => defmt::info!("rx: {=str}", text),
                        Err(_) => defmt::info!("rx: {=[u8]:x}", &line[..end]),
                    }
                    len = 0;
                } else {
                    line[len] = byte[0];
                    len += 1;
                }
            }
            // `LSR` reports which line-status error fired; an overrun means the
            // echo could not keep up with the sender.
            Err(Error::Overrun) => defmt::warn!("uart overrun"),
            Err(Error::Parity) => defmt::warn!("uart parity error"),
            Err(Error::Framing) => defmt::warn!("uart framing error"),
            Err(Error::Break) => defmt::warn!("uart break"),
        }
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    // 9600, not the crate-wide 115200 default: this board's clock cannot
    // generate 115200 closely enough for a real adapter, see the module docs.
    let config = Config::new(9600);
    let baudrate = config.baudrate;
    let uart = Uart::new::<0>(p.UART3, p.PA4, p.PA5, Irqs, config).unwrap();

    // `Executor::new` is not `const`, so it must be built at runtime. This
    // function never returns, so the storage lives for the rest of the program.
    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(echo(uart, baudrate).unwrap());
    })
}
