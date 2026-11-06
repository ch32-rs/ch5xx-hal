//! Enumerate as a USB CDC ACM serial port and echo what the host sends.
//!
//! The full-speed device is on PB11 (D+) and PB10 (D-), connected to the host
//! through the board's USB device connector. `PIN_USB_EN` routes the pins, so
//! they are not GPIO here. The clock is the 78 MHz HSI PLL; no crystal is needed.
//!
//! The device enumerates as a CDC ACM port. Opening it and typing echoes each
//! packet back and logs the length over RTT. The interrupt handler is bound to
//! `USB` through `bind_interrupts!`, and the device task runs on the same
//! executor as `uart_echo`.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use embassy_executor::Spawner;
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
        spawner.spawn(echo(class, spawner).unwrap());
    })
}

#[embassy_executor::task]
async fn echo(mut class: CdcAcmClass<'static, CdcDriver>, _spawner: Spawner) -> ! {
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
        defmt::info!("cdc rx {} bytes", n);
        class.write_packet(&buf[..n]).await?;
    }
}
