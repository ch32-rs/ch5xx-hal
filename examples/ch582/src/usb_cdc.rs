use defmt_rtt as _;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::{Builder, UsbDevice};
use hal::usb::Driver;
use panic_halt as _;
use qingke_rt::entry;

use super::{controller, hal, Controller, Irqs};

type CdcDriver = Driver<'static, Controller>;

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, CdcDriver>) -> ! {
    device.run().await
}

#[embassy_executor::task]
async fn echo(mut class: CdcAcmClass<'static, CdcDriver>) -> ! {
    let mut buf = [0; 64];
    loop {
        class.wait_connection().await;
        while let Ok(n) = class.read_packet(&mut buf).await {
            if class.write_packet(&buf[..n]).await.is_err() {
                break;
            }
        }
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(hal::Config {
        rcc: hal::rcc::Config::PLL_48MHZ,
    });
    let driver = Driver::new(controller(p), Irqs);
    let mut config = embassy_usb::Config::new(0x1a86, 0x55f4);
    config.manufacturer = Some("ch5xx-hal");
    config.product = Some("CH582 CDC echo");
    config.serial_number = Some("0001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static mut CONFIG: [u8; 256] = [0; 256];
    static mut BOS: [u8; 256] = [0; 256];
    static mut MSOS: [u8; 0] = [];
    static mut CONTROL: [u8; 64] = [0; 64];
    static mut STATE: Option<State<'static>> = None;
    let mut builder = Builder::new(
        driver,
        config,
        unsafe { &mut *core::ptr::addr_of_mut!(CONFIG) },
        unsafe { &mut *core::ptr::addr_of_mut!(BOS) },
        unsafe { &mut *core::ptr::addr_of_mut!(MSOS) },
        unsafe { &mut *core::ptr::addr_of_mut!(CONTROL) },
    );
    let state = unsafe { &mut *core::ptr::addr_of_mut!(STATE) }.insert(State::new());
    let class = CdcAcmClass::new(&mut builder, state, 64);
    let usb = builder.build();

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    let executor = unsafe { &mut *core::ptr::addr_of_mut!(EXECUTOR) }.insert(embassy_executor::Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(usb_task(usb).unwrap());
        spawner.spawn(echo(class).unwrap());
    })
}
