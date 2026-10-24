//! Log to the host with `defmt` over SEGGER RTT.
//!
//! RTT is a RAM-based ring buffer that the debugger reads out of memory, so it
//! needs no UART pin and no special chip support — unlike SDI printf, which the
//! CH585 does not have.
//!
//! ```console
//! cargo run --release --bin defmt_rtt
//! ```
//!
//! or, separately:
//!
//! ```console
//! probe-rs download --chip CH585 target/riscv32imac-unknown-none-elf/release/defmt_rtt
//! probe-rs attach --chip CH585 target/riscv32imac-unknown-none-elf/release/defmt_rtt
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;

use defmt_rtt as _;
use {defmt::info, panic_halt as _, qingke_rt::entry};

#[entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    info!("ch5xx-hal defmt/RTT demo");
    info!("measured sysclk: {} Hz", hal::rcc::clocks().sysclk.0);

    let mut delay = hal::delay::Delay;
    let mut n: u32 = 0;

    loop {
        info!("tick {}", n);
        n = n.wrapping_add(1);
        delay.delay_ms(500);
    }
}
