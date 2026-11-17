//! PWMX self-check with no jumpers: drive a channel and read the pad back
//! through the GPIO input path, so the high-time ratio is measured directly.
//!
//! PA4/PWM1, PB1/PWM2 and PB2/PWM3 are plain pins; PB15/PWM0 is skipped because
//! it is the external-reset pad. Results at 0x20021000, 20 words:
//!
//! ```text
//! [0]      "PWMX"
//! [1..3]   256-cycle, active high, duty 64  (duty, high, total)
//! [4..6]   ... duty 128
//! [7..9]   ... duty 192
//! [10..12] 256-cycle, active low, duty 64 (PB1; expect ~75% high)
//! [13..15] 255-cycle, active high, duty 64 (PB2)
//! [16] CTRL_MOD, [17] CTRL_CFG, [18] CLOCK_DIV, [19] Fsys Hz
//! ```
//!
//! ```console
//! cargo build --release --bin pwmx_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 20
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pwm::{Config, Cycle, Polarity, Pwm};
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const N: u32 = 1 << 20;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn high_pa(pin: usize) -> u32 {
    let mut high = 0;
    for _ in 0..N {
        high += hal::pac::GPIO.pa_pin().read().pa_pin(pin) as u32;
    }
    high
}

fn high_pb(pin: usize) -> u32 {
    let mut high = 0;
    for _ in 0..N {
        high += hal::pac::GPIO.pb_pin().read().pb_pin(pin) as u32;
    }
    high
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = hal::delay::Delay;

    scratch(0, u32::from_le_bytes(*b"PWMX"));

    let pwm = Pwm::new(p.PWMX, Config::new(4, Cycle::Clocks256)).unwrap();
    {
        let mut ch = pwm.channel(p.PA4, Polarity::ActiveHigh);
        for (i, duty) in [64u16, 128, 192].into_iter().enumerate() {
            ch.set_duty(duty);
            delay.delay_ms(2);
            scratch(1 + i * 3, duty as u32);
            scratch(2 + i * 3, high_pa(4));
            scratch(3 + i * 3, N);
        }
    }
    {
        let mut ch = pwm.channel(p.PB1, Polarity::ActiveLow);
        ch.set_duty(64);
        delay.delay_ms(2);
        scratch(10, 64);
        scratch(11, high_pb(1));
        scratch(12, N);
    }

    // Switch the shared cycle to 255 and measure the third channel there.
    hal::pac::PWMX.ctrl_cfg().modify(|w| w.set_cycle_sel(true));
    {
        let mut ch = pwm.channel(p.PB2, Polarity::ActiveHigh);
        ch.set_duty(64);
        delay.delay_ms(2);
        scratch(13, 64);
        scratch(14, high_pb(2));
        scratch(15, N);
    }

    scratch(16, hal::pac::PWMX.ctrl_mod().read().0 as u32);
    scratch(17, hal::pac::PWMX.ctrl_cfg().read().0 as u32);
    scratch(18, hal::pac::PWMX.clock_div().read().clock_div() as u32);
    scratch(19, hal::rcc::clocks().sysclk.0 as u32);

    loop {
        core::hint::spin_loop();
    }
}
