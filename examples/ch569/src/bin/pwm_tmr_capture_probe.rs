//! Cross-check PWMX against TMR2 input capture.
//!
//! Wire P3-37 (PB2, PWM3) to P3-20 (PA4, TMR2): PWM3 runs at Fsys/4 over a
//! 256-clock cycle with 50% duty, and TMR2 counts the rising-edge intervals.
//! The first captures go to UART1 (J1) so no debugger attach is needed.
//!
//! ```console
//! cargo build --release --bin pwm_tmr_capture_probe
//! wlink -d 1a86:8010:<SERIAL> flash -e target/riscv32imac-unknown-none-elf/release/pwm_tmr_capture_probe
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::gpio::Pull;
use ch5xx_hal::mode::Blocking;
use ch5xx_hal::pwm::{Config as PwmConfig, Cycle, Polarity, Pwm};
use ch5xx_hal::timer::input_capture::InputCapture;
use ch5xx_hal::timer::{CaptureEdge, PeriodTicks};
use ch5xx_hal::uart::{Config, Uart, UartTx};
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

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = hal::delay::Delay;
    let uart = Uart::new_blocking(p.UART1, p.PA7, p.PA8, Config::new(115_200)).unwrap();
    let (mut tx, _rx) = uart.split();
    let _ = tx.blocking_write(b"\r\nCH569 PWMX->TMR2 READY\r\n");

    scratch(0, u32::from_le_bytes(*b"PTCK"));
    for i in 1..8 {
        scratch(i, 0);
    }

    // 15 MHz / 4 = 3.75 MHz channel clock; 256 clocks per period.
    let pwm = Pwm::new(p.PWMX, PwmConfig::new(4, Cycle::Clocks256)).unwrap();
    let mut ch = pwm.channel(p.PB2, Polarity::ActiveHigh);
    ch.set_duty(128);

    // Read both pads back first: PB2 shows the driver works, PA4 shows the wire.
    // A pull-down first: a floating PA4 would read ~0 and mimic nothing, while
    // a real connection still reads ~50%.
    hal::pac::GPIO.pa_pu().modify(|w| w.set_pa_pu(4, false));
    hal::pac::GPIO.pa_pd().modify(|w| w.set_pa_pd(4, true));
    const N: u32 = 1 << 20;
    let mut pb2_high = 0u32;
    let mut pa4_high = 0u32;
    for _ in 0..N {
        pb2_high += hal::pac::GPIO.pb_pin().read().pb_pin(2) as u32;
        pa4_high += hal::pac::GPIO.pa_pin().read().pa_pin(4) as u32;
    }
    scratch(10, pb2_high);
    scratch(11, pa4_high);
    scratch(12, N);
    for (label, v) in [(b"PB2", pb2_high), (b"PA4", pa4_high)] {
        let _ = tx.blocking_write(label);
        let _ = tx.blocking_write(b"_high=");
        write_u32(&mut tx, v);
        let _ = tx.blocking_write(b"/");
        write_u32(&mut tx, N);
        let _ = tx.blocking_write(b"\r\n");
    }

    let mut cap = InputCapture::new(
        p.TMR2,
        p.PA4,
        Pull::Up,
        PeriodTicks::new_capture(16_384).unwrap(),
        CaptureEdge::RisingToRising,
    );
    // Re-check forever: fitting the jumper while the probe runs shows up in the
    // next burst. A burst is eight captured periods collected before printing,
    // so a slow UART cannot overrun the capture FIFO.
    let mut timeouts = 0u32;
    let mut errors = 0u32;
    // The counter restarts mid-period, so the first capture of a burst is a
    // fragment; drop it.
    let mut skip = 1u8;
    loop {
        let mut words = [0u32; 8];
        let mut got = 0usize;
        while got < 8 {
            match cap.try_capture() {
                Ok(Some(word)) if word.0 & 0x0200_0000 == 0 => {
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    words[got] = word.0;
                    got += 1;
                }
                Ok(Some(_)) => {
                    timeouts = timeouts.wrapping_add(1);
                    if timeouts % 2000 == 0 {
                        let _ = tx.blocking_write(b"no edges yet\r\n");
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    // FIFO overrun: acknowledge the flag and keep draining.
                    errors = errors.wrapping_add(1);
                    hal::pac::TMR2.int_flag().write(|w| w.0 = 16);
                }
            }
        }
        scratch(9, timeouts);
        scratch(10, errors);
        for (i, word) in words.iter().enumerate() {
            scratch(1 + i, *word);
            let _ = tx.blocking_write(b"interval=");
            write_u32(&mut tx, word & 0x01ff_ffff);
            let _ = tx.blocking_write(b"\r\n");
        }
        let _ = tx.blocking_write(b"burst done, timeouts=");
        write_u32(&mut tx, timeouts);
        let _ = tx.blocking_write(b" errors=");
        write_u32(&mut tx, errors);
        let _ = tx.blocking_write(b"\r\n");

        // Park the counter so the print cannot overflow the FIFO.
        hal::pac::TMR2.ctrl_mod().modify(|w| w.0 &= !4);
        delay.delay_ms(500);
        hal::pac::TMR2.ctrl_mod().modify(|w| w.0 |= 4);
        skip = 1;
    }
}
