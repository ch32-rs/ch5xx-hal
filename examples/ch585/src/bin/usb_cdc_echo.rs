//! Enumerate as a USB CDC ACM serial port and exercise the full-speed endpoints.
//!
//! The full-speed device is on PB11 (D+) and PB10 (D-), connected to the host
//! through the board's FS-USB connector. `PIN_USB_EN` routes the pins, so they
//! are not GPIO here. The clock is the 78 MHz HSI PLL; no crystal is needed.
//!
//! Ordinary packets are echoed unchanged. Two exact-packet commands help a host
//! validation script: `PING` returns `PONG`, and `PATTERN <n>` returns `n` bytes
//! counting modulo 256. Pattern responses and ordinary binary echo cover both
//! packet boundaries and sustained transfers. The interrupt handler is bound to
//! `USB` through `bind_interrupts!`.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::{Builder, UsbDevice};
use hal::peripherals::USBFS;
use hal::rcc::Config as ClockConfig;
use hal::usb::{Driver, InterruptHandler};
use panic_halt as _;
use qingke_rt::entry;

hal::bind_interrupts!(struct Irqs {
    USB => InterruptHandler<USBFS>;
});

type CdcDriver = Driver<'static, USBFS>;

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, CdcDriver>) -> ! {
    device.run().await
}

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: ClockConfig::HSI_PLL_78MHZ,
    });

    let driver = Driver::new(p.USBFS, Irqs);
    defmt::info!("usb cdc echo ready");

    let mut config = embassy_usb::Config::new(0x1a86, 0x55f0);
    config.manufacturer = Some("ch5xx-hal");
    config.product = Some("CH585 CDC echo");
    config.serial_number = Some("0001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // The device task outlives this stack frame, so the descriptor buffers and
    // the class state are static, like the executor below.
    static mut CONFIG_DESCRIPTOR: [u8; 256] = [0; 256];
    static mut BOS_DESCRIPTOR: [u8; 256] = [0; 256];
    static mut MSOS_DESCRIPTOR: [u8; 0] = [];
    static mut CONTROL_BUF: [u8; 64] = [0; 64];
    static mut STATE: Option<State<'static>> = None;

    let config_descriptor = unsafe { &mut *core::ptr::addr_of_mut!(CONFIG_DESCRIPTOR) };
    let bos_descriptor = unsafe { &mut *core::ptr::addr_of_mut!(BOS_DESCRIPTOR) };
    let msos_descriptor = unsafe { &mut *core::ptr::addr_of_mut!(MSOS_DESCRIPTOR) };
    let control_buf = unsafe { &mut *core::ptr::addr_of_mut!(CONTROL_BUF) };
    let state = unsafe { &mut *core::ptr::addr_of_mut!(STATE) }.insert(State::new());
    let mut builder = Builder::new(
        driver,
        config,
        config_descriptor,
        bos_descriptor,
        msos_descriptor,
        control_buf,
    );

    let class = CdcAcmClass::new(&mut builder, state, 64);
    let usb = builder.build();

    // `Executor::new` is not `const`, so it must be built at runtime. This
    // function never returns, so the storage lives for the rest of the program.
    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(usb_task(usb).unwrap());
        spawner.spawn(echo(class).unwrap());
    })
}

#[embassy_executor::task]
async fn echo(mut class: CdcAcmClass<'static, CdcDriver>) -> ! {
    loop {
        class.wait_connection().await;
        defmt::info!("cdc connected");
        let _ = echo_packets(&mut class).await;
        defmt::info!("cdc disconnected");
    }
}

async fn echo_packets(class: &mut CdcAcmClass<'static, CdcDriver>) -> Result<(), EndpointError> {
    let mut buf = [0u8; 64];
    loop {
        let n = class.read_packet(&mut buf).await?;
        let packet = &buf[..n];
        if packet == b"PING" {
            write_packets(class, b"PONG").await?;
        } else if let Some(len) = pattern_length(packet) {
            defmt::info!("cdc pattern {} bytes", len);
            write_pattern(class, len).await?;
        } else {
            write_packets(class, packet).await?;
        }
    }
}

/// Write a whole response in full-speed packets.
async fn write_packets(class: &mut CdcAcmClass<'static, CdcDriver>, mut data: &[u8]) -> Result<(), EndpointError> {
    while !data.is_empty() {
        let len = data.len().min(64);
        class.write_packet(&data[..len]).await?;
        data = &data[len..];
    }
    Ok(())
}

/// `PATTERN <decimal length>`, bounded so a typo cannot run forever.
fn pattern_length(packet: &[u8]) -> Option<usize> {
    let digits = packet.strip_prefix(b"PATTERN ")?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut len = 0usize;
    for digit in digits {
        len = len.checked_mul(10)?.checked_add((digit - b'0') as usize)?;
    }
    (len <= 65_536).then_some(len)
}

async fn write_pattern(class: &mut CdcAcmClass<'static, CdcDriver>, len: usize) -> Result<(), EndpointError> {
    let mut offset = 0;
    let mut packet = [0u8; 64];
    while offset < len {
        let count = (len - offset).min(packet.len());
        for (index, byte) in packet[..count].iter_mut().enumerate() {
            *byte = (offset + index) as u8;
        }
        class.write_packet(&packet[..count]).await?;
        offset += count;
    }
    Ok(())
}
