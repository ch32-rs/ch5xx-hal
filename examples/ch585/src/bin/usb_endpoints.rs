//! Vendor-class USBFS device that exercises every endpoint, 1 to 7.
//!
//! Three bulk echo pairs (`EP2 -> EP1`, `EP4 -> EP3`, `EP6 -> EP5`) and one
//! pattern source on `EP7 IN` cover each endpoint index once. This includes
//! endpoint 4, whose OUT and IN buffers follow endpoint 0's, and endpoints 5 to
//! 7, which are enabled through `UEP567_MOD` with their own DMA registers.
//!
//! The class is vendor-specific, so no host driver claims the interface and a
//! libusb script can drive it directly. A packet written to `EP2` comes back on
//! `EP1`; `EP4`/`EP3` and `EP6`/`EP5` behave the same way. `EP7` streams
//! packets whose first three bytes are `0xE7`, a counter high byte and low byte.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use embassy_usb::driver::{Direction, Endpoint, EndpointAddress, EndpointIn, EndpointOut};
use embassy_usb::{Builder, UsbDevice};
use hal::peripherals::USBFS;
use hal::usb::{Driver, In, Out};
use panic_halt as _;
use qingke_rt::entry;

hal::bind_interrupts!(struct Irqs {
    USB => hal::usb::InterruptHandler<USBFS>;
});

type UsbDriver = Driver<'static, USBFS>;
type UsbIn = hal::usb::Endpoint<'static, USBFS, In>;
type UsbOut = hal::usb::Endpoint<'static, USBFS, Out>;

const PACKET: usize = 64;

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: hal::rcc::Config::HSI_PLL_78MHZ,
    });

    let driver = Driver::new(p.USBFS, Irqs);

    let mut config = embassy_usb::Config::new(0x1a86, 0x55f2);
    config.manufacturer = Some("ch5xx-hal");
    config.product = Some("CH585 endpoint test");
    config.serial_number = Some("0003");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static mut CONFIG_DESCRIPTOR: [u8; 512] = [0; 512];
    static mut BOS_DESCRIPTOR: [u8; 256] = [0; 256];
    static mut MSOS_DESCRIPTOR: [u8; 0] = [];
    static mut CONTROL_BUF: [u8; 64] = [0; 64];

    let config_descriptor = unsafe { &mut *core::ptr::addr_of_mut!(CONFIG_DESCRIPTOR) };
    let bos_descriptor = unsafe { &mut *core::ptr::addr_of_mut!(BOS_DESCRIPTOR) };
    let msos_descriptor = unsafe { &mut *core::ptr::addr_of_mut!(MSOS_DESCRIPTOR) };
    let control_buf = unsafe { &mut *core::ptr::addr_of_mut!(CONTROL_BUF) };
    let mut builder = Builder::new(
        driver,
        config,
        config_descriptor,
        bos_descriptor,
        msos_descriptor,
        control_buf,
    );

    let (ep1_in, ep2_out, ep3_in, ep4_out, ep5_in, ep6_out, ep7_in) = {
        let mut function = builder.function(0xff, 0x00, 0x00);
        let mut interface = function.interface();
        let mut alt = interface.alt_setting(0xff, 0x00, 0x00, None);
        // Explicit addresses: the host script addresses exactly these
        // endpoints, and the driver is asked for each direction separately.
        let ep1_in = alt.endpoint_bulk_in(Some(address(1, Direction::In)), PACKET as u16);
        let ep2_out = alt.endpoint_bulk_out(Some(address(2, Direction::Out)), PACKET as u16);
        let ep3_in = alt.endpoint_bulk_in(Some(address(3, Direction::In)), PACKET as u16);
        let ep4_out = alt.endpoint_bulk_out(Some(address(4, Direction::Out)), PACKET as u16);
        let ep5_in = alt.endpoint_bulk_in(Some(address(5, Direction::In)), PACKET as u16);
        let ep6_out = alt.endpoint_bulk_out(Some(address(6, Direction::Out)), PACKET as u16);
        let ep7_in = alt.endpoint_bulk_in(Some(address(7, Direction::In)), PACKET as u16);
        (ep1_in, ep2_out, ep3_in, ep4_out, ep5_in, ep6_out, ep7_in)
    };
    let usb = builder.build();

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    defmt::info!("usb endpoints ready");
    executor.run(move |spawner| {
        spawner.spawn(usb_task(usb).unwrap());
        spawner.spawn(echo(ep2_out, ep1_in).unwrap());
        spawner.spawn(echo(ep4_out, ep3_in).unwrap());
        spawner.spawn(echo(ep6_out, ep5_in).unwrap());
        spawner.spawn(pattern(ep7_in).unwrap());
    })
}

fn address(index: u8, direction: Direction) -> EndpointAddress {
    EndpointAddress::from_parts(index as usize, direction)
}

/// Echo every packet from `out` back on `in_`.
///
/// Three instances run at once, so the task needs a pool of three; the default
/// pool of one makes the second spawn fail and the panic keeps the executor
/// from ever starting.
#[embassy_executor::task(pool_size = 3)]
async fn echo(mut out: UsbOut, mut in_: UsbIn) -> ! {
    let mut buffer = [0u8; PACKET];
    loop {
        match out.read(&mut buffer).await {
            Ok(len) => {
                if in_.write(&buffer[..len]).await.is_err() {
                    in_.wait_enabled().await;
                }
            }
            Err(_) => {
                // A bus reset disables the endpoints until the host selects the
                // configuration again.
                out.wait_enabled().await;
                in_.wait_enabled().await;
            }
        }
    }
}

/// Stream tagged packets on the last endpoint while the host polls it.
#[embassy_executor::task]
async fn pattern(mut ep: UsbIn) -> ! {
    let mut packet = [0u8; PACKET];
    let mut counter = 0u16;
    loop {
        packet[0] = 0xE7;
        packet[1] = (counter >> 8) as u8;
        packet[2] = counter as u8;
        for (index, byte) in packet[3..].iter_mut().enumerate() {
            *byte = (index as u16).wrapping_add(counter) as u8;
        }
        if ep.write(&packet).await.is_err() {
            ep.wait_enabled().await;
        }
        counter = counter.wrapping_add(1);
    }
}
