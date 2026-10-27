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
//! This example runs **115200 8N1** with the clock raised to
//! `rcc::Config::HSE_PLL_78MHZ`, which uses the board's 32 MHz crystal. The
//! divider becomes `DL = 85`, i.e. 114706 baud (-0.43%), inside the datasheet's
//! 0.5% transmit budget. At the reset clock (5.333 MHz) 115200 cannot be
//! generated closely enough (`DL = 6` is 111.1 kbaud, -3.5%), so the clock is
//! configured before the UART is built.
//!
//! `defmt` writes hold a critical section, so a line log delays the next read by
//! a few milliseconds; a sender that keeps transmitting during the log can
//! overrun the eight-byte FIFO. For interactive traffic that does not matter.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::rcc;
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
    let p = hal::init(rcc::Config::HSE_PLL_78MHZ.into());

    // 115200 8N1, the crate-wide default, now that `Fsys` is high enough for
    // the divider to land inside the transmit budget; see the module docs.
    let config = Config::default();
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
