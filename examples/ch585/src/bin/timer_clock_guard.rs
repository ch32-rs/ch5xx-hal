//! NFC clock precondition at 39 MHz, configured at boot, not under a live timebase.
#![no_std]
#![no_main]
use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::timer::low_level::Timer;
use hal::timer::{Error, NfcClockInstance, PeriodTicks};
use panic_halt as _;

#[no_mangle]
pub static mut CLOCK_RESULTS: [u32; 8] = [0; 8];
fn record(n: usize, v: u32) {
    unsafe {
        core::ptr::addr_of_mut!(CLOCK_RESULTS)
            .cast::<u32>()
            .add(n)
            .write_volatile(v)
    }
}
fn check<T: NfcClockInstance>(t: hal::Peri<'_, T>, id: usize, r: hal::pac::tmr::TmrFull) {
    let mut timer = Timer::new(t);
    timer.configure_counter(PeriodTicks::new(78000).unwrap());
    let before = r.ctrl_mod().read().0;
    assert!(timer.set_nfc_clock() == Err(Error::ClockTooSlow));
    assert!(r.ctrl_mod().read().0 == before && before & 0x20 == 0);
    record(id, 1);
}
#[qingke_rt::entry]
fn main() -> ! {
    let p = hal::init(hal::rcc::Config::HSE_PLL_39MHZ.into());
    record(1, hal::rcc::clocks().sysclk.0);
    #[cfg(not(feature = "time-driver-tmr0"))]
    check(p.TMR0, 2, hal::pac::TMR0);
    #[cfg(not(feature = "time-driver-tmr3"))]
    check(p.TMR3, 3, hal::pac::TMR3);
    record(0, 0xc0de5852);
    loop {
        core::hint::spin_loop()
    }
}
