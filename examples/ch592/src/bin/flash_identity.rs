//! Read the factory MAC address and unique ID through the flash engine.
//!
//! ```console
//! PROBE_RS_PROBE=1a86:8010:7E238F0602BE cargo run --release --bin flash_identity --features flash
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::flash::{Error, Flash};
use defmt_rtt as _;
use panic_halt as _;
use qingke_rt::entry;

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut flash = Flash::new(p.FLASHROM);

    match flash.mac_address() {
        Ok(mac) => defmt::info!("factory mac: {=[u8]:02x}", mac),
        Err(err) => defmt::error!("FAIL mac: {}", what(err)),
    }
    match flash.unique_id() {
        Ok(id) => defmt::info!("unique id: {:#018x}", id),
        Err(err) => defmt::error!("FAIL unique id: {}", what(err)),
    }
    defmt::info!("PASS identity");

    // Stay halted without the watchdog resetting the part; a reset would clear
    // the RTT block and the log with it.
    loop {
        hal::pac::SYS.iwdg_kr().write_value(hal::pac::sys::regs::IwdgKr(0xAAAA));
    }
}

fn what(err: Error) -> &'static str {
    match err {
        Error::NotAligned => "not aligned",
        Error::OutOfBounds => "out of bounds",
        Error::Timeout => "timeout",
        Error::Rejected => "rejected",
        Error::Unsupported => "unsupported die",
    }
}
