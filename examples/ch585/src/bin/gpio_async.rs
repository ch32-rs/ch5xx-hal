//! Report each edge of the two board buttons through the port interrupt and
//! mirror each button on its LED.
//!
//! S1 is PB22 and S2 is PB23. Both short the pin to GND and idle high on the
//! internal pull-up, so a press is a falling edge and a release is a rising
//! one. PB22/PB23 have no flag bits of their own: the port-B interrupt registers
//! are 16 bits wide, and bits 9:8 follow these two pins while `PIN_INTX` is set.
//! PB8/PB9 cannot be watched at the same time.
//!
//! LED0 is PB16 and LED1 is PB17. Both LEDs share a polarity and the two pins
//! rest at opposite levels (PB16 low, PB17 high), so exactly one of them is lit.
//! S1 drives PB16 high while held and S2 drives PB17 low, so pressing either
//! button makes both pins share a level and both LEDs become lit or dark;
//! releasing it restores the resting pattern. The result is visible without
//! RTT whichever LED is the active-high one.
//!
//! Nothing is debounced. One press can log several edges; that is the contact,
//! not the interrupt. A level wait would return immediately while the button
//! rests high, so this waits for edges only.
//!
//! `defmt` over RTT prints one line per edge in addition to the LED feedback.
//! The port interrupts are claimed by `hal::init` when the `embassy` feature is
//! on, so there is nothing to bind.
//!
//! Both buttons run the same task, so the pool holds two instances. The default
//! pool size is one, and spawning the second button would then return `Busy` and
//! the `.unwrap()` would halt the program before the first edge.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::gpio::{Input, Level, Output, Pull, Speed};
use panic_halt as _;
use qingke_rt::entry;

#[embassy_executor::task(pool_size = 2)]
async fn button(
    name: &'static str,
    mut pin: Input<'static>,
    mut led: Output<'static>,
    pressed: Level,
    released: Level,
) -> ! {
    loop {
        pin.wait_for_falling_edge().await;
        defmt::info!("{=str} pressed", name);
        led.set_level(pressed);
        pin.wait_for_rising_edge().await;
        defmt::info!("{=str} released", name);
        led.set_level(released);
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let s1 = Input::new(p.PB22, Pull::Up);
    let s2 = Input::new(p.PB23, Pull::Up);
    // Resting pattern: PB16 low, PB17 high. One of the two LEDs is lit.
    let led0 = Output::new(p.PB16, Level::Low, Speed::Low);
    let led1 = Output::new(p.PB17, Level::High, Speed::Low);
    defmt::info!("S1=PB22 S2=PB23, waiting for edges");

    // `Executor::new` is not `const`, so it must be built at runtime. This
    // function never returns, so the storage lives for the rest of the program.
    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner
            .spawn(button("S1", s1, led0, Level::High, Level::Low).unwrap());
        spawner
            .spawn(button("S2", s2, led1, Level::Low, Level::High).unwrap());
    })
}
