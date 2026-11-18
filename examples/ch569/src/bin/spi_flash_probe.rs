//! Read the board's W25Q32JV over SPI0: JEDEC ID, status and SFDP.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::gpio::Level;
use ch5xx_hal::mode::Blocking;
use ch5xx_hal::spi::{chip_select, Config, Spi};
use ch5xx_hal::time::Hertz;
use ch5xx_hal::uart::{Config as UartConfig, Uart, UartTx};
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn write_str(tx: &mut UartTx<'static, hal::peripherals::UART1, Blocking>, s: &[u8]) {
    let _ = tx.blocking_write(s);
}

fn write_hex(tx: &mut UartTx<'static, hal::peripherals::UART1, Blocking>, v: u32, digits: usize) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for i in (0..digits).rev() {
        let _ = tx.blocking_write(&[HEX[((v >> (i * 4)) & 0xf) as usize]]);
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = hal::delay::Delay;
    let uart = Uart::new_blocking(p.UART1, p.PA7, p.PA8, UartConfig::new(115_200)).unwrap();
    let (mut tx, _rx) = uart.split();
    write_str(&mut tx, b"\r\nCH569 SPI0 FLASH READY\r\n");

    // Fsys 15 MHz / 4 = 3.75 MHz SCK.
    let mut spi = Spi::new_blocking(p.SPI0, p.PA13, p.PA14, p.PA15, Config::new(Hertz(3_750_000))).unwrap();
    let mut cs = chip_select(p.PA12, Level::High);

    // JEDEC ID (0x9F); full duplex puts the command phase in read slot 0.
    let mut frame = [0u8; 4];
    cs.select(|| {
        let _ = spi.blocking_transfer(&mut frame, &[0x9F]);
    });
    let id_word = (u32::from(frame[1]) << 16) | (u32::from(frame[2]) << 8) | u32::from(frame[3]);
    scratch(0, u32::from_le_bytes(*b"FLSH"));
    scratch(1, id_word);
    write_str(&mut tx, b"jedec=");
    write_hex(&mut tx, id_word, 6);
    write_str(&mut tx, b"\r\n");

    // Status register 1, command 0x05; same one-byte command-phase offset.
    let mut frame = [0u8; 2];
    cs.select(|| {
        let _ = spi.blocking_transfer(&mut frame, &[0x05]);
    });
    let status = frame[1];
    scratch(2, u32::from(status));
    write_str(&mut tx, b"status=");
    write_hex(&mut tx, u32::from(status), 2);
    write_str(&mut tx, b"\r\n");

    // SFDP header, command 0x5A: address 0 then one dummy byte, and the four
    // data bytes spell "SFDP". Blank or absent flash cannot fake this.
    let mut sfdp = [0u8; 5];
    cs.select(|| {
        let _ = spi.blocking_write(&[0x5A, 0, 0, 0]);
        let _ = spi.blocking_read(&mut sfdp);
    });
    let sig = u32::from_be_bytes([sfdp[1], sfdp[2], sfdp[3], sfdp[4]]);
    scratch(3, sig);
    write_str(&mut tx, b"sfdp=");
    write_hex(&mut tx, sig, 8);
    write_str(&mut tx, b" (");
    write_str(&mut tx, &sfdp[1..5]);
    write_str(&mut tx, b")\r\n");

    // Read 16 bytes, command 0x03, 24-bit address MSB first.
    let mut buf = [0u8; 16];
    for (round, base) in [(0u32, 0x000000u32), (1, 0x000100), (2, 0x3FF000)] {
        cs.select(|| {
            let _ = spi.blocking_write(&[0x03, (base >> 16) as u8, (base >> 8) as u8, base as u8]);
            let _ = spi.blocking_read(&mut buf);
        });
        scratch(4 + round as usize, base);
        write_str(&mut tx, b"addr=");
        write_hex(&mut tx, base, 6);
        write_str(&mut tx, b": ");
        for byte in buf.iter() {
            write_hex(&mut tx, u32::from(*byte), 2);
            write_str(&mut tx, b" ");
        }
        write_str(&mut tx, b"\r\n");
        scratch(8 + round as usize, u32::from_le_bytes(buf[..4].try_into().unwrap()));
    }
    write_str(&mut tx, b"DONE\r\n");

    loop {
        delay.delay_ms(2000);
        write_str(&mut tx, b"alive\r\n");
    }
}
