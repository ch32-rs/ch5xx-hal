//! Detached version of the 500 ms async / 100 ms blocking timebase regression.
#![no_std]
#![no_main]
use ch5xx_hal as hal;
use defmt_rtt as _;
use panic_halt as _;

#[no_mangle]
pub static mut TIMEBASE_RESULTS: [u32; 40] = [0; 40];
fn record(n: usize, value: u32) {
    unsafe {
        core::ptr::addr_of_mut!(TIMEBASE_RESULTS)
            .cast::<u32>()
            .add(n)
            .write_volatile(value)
    }
}
fn rtc() -> u16 {
    hal::pac::RTC.cnt_32k().read().cnt_32k()
}
#[embassy_executor::task]
async fn exercise() {
    let _p = hal::init(Default::default());
    #[cfg(feature = "time-driver-systick")]
    record(2, 0);
    #[cfg(feature = "time-driver-tmr0")]
    record(2, 1);
    #[cfg(feature = "time-driver-tmr1")]
    record(2, 2);
    #[cfg(feature = "time-driver-tmr2")]
    record(2, 3);
    #[cfg(feature = "time-driver-tmr3")]
    record(2, 4);
    for n in 0..8 {
        record(1, n as u32);
        let a = rtc();
        let start = embassy_time::Instant::now();
        embassy_time::Timer::after_millis(500).await;
        record(4 + n, start.elapsed().as_millis() as u32);
        record(20 + n, rtc().wrapping_sub(a) as u32);
        let a = rtc();
        let start = embassy_time::Instant::now();
        hal::delay::Delay.delay_ms(100);
        record(12 + n, start.elapsed().as_millis() as u32);
        record(28 + n, rtc().wrapping_sub(a) as u32);
    }
    record(1, 100);
    record(0, 0xc0de5853);
    core::future::pending::<()>().await;
}
#[qingke_rt::entry]
fn main() -> ! {
    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };
    executor.run(|spawner| {
        spawner.spawn(exercise().unwrap());
    })
}
