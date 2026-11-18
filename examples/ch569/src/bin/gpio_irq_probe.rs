//! Pin interrupt through the single-vector GPIO block (wire PB1 to PA4).

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::gpio::{Input, Level, Output, Pull, Speed};
use ch5xx_hal::mode::Blocking;
use ch5xx_hal::uart::{Config, Uart, UartTx};
use embassy_time::{Duration, Timer};
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn write_u32(tx: &mut UartTx<'static, hal::peripherals::UART1, Blocking>, mut v: u32) {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let _ = tx.blocking_write(&buf[i..]);
}

#[embassy_executor::task]
async fn drive(mut pin: Output<'static>) {
    let mut toggles = 0u32;
    loop {
        pin.toggle();
        toggles = toggles.wrapping_add(1);
        scratch(1, toggles);
        Timer::after(Duration::from_millis(2)).await;
    }
}

#[embassy_executor::task]
async fn watch(mut pin: Input<'static>, mut tx: UartTx<'static, hal::peripherals::UART1, Blocking>) {
    let mut rise = 0u32;
    let mut fall = 0u32;
    loop {
        pin.wait_for_rising_edge().await;
        rise = rise.wrapping_add(1);
        scratch(2, rise);
        if rise <= 3 || rise % 100 == 0 {
            let _ = tx.blocking_write(b"IRQ rise=");
            write_u32(&mut tx, rise);
            let _ = tx.blocking_write(b" fall=");
            write_u32(&mut tx, fall);
            let _ = tx.blocking_write(b"\r\n");
        }
        pin.wait_for_falling_edge().await;
        fall = fall.wrapping_add(1);
        scratch(3, fall);
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let uart = Uart::new_blocking(p.UART1, p.PA7, p.PA8, Config::new(115_200)).unwrap();
    let (mut tx, _rx) = uart.split();
    let _ = tx.blocking_write(b"\r\nCH569 IRQ READY (PB1 -> PA4)\r\n");

    scratch(0, u32::from_le_bytes(*b"IRQP"));
    let pb1 = Output::new(p.PB1, Level::Low, Speed::Low);
    let pa4 = Input::new(p.PA4, Pull::Down);

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(drive(pb1).unwrap());
        spawner.spawn(watch(pa4, tx).unwrap());
    })
}
