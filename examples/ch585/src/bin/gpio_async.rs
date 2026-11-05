//! Report each edge of the two board buttons through the port interrupt.
//!
//! S1 is PB22 and S2 is PB23. Both short the pin to GND and idle high on the
//! internal pull-up, so a press is a falling edge and a release is a rising
//! one. PB22/PB23 have no flag bits of their own: the port-B interrupt registers
//! are 16 bits wide, and bits 9:8 follow these two pins while `PIN_INTX` is set.
//! PB8/PB9 cannot be watched at the same time.
//!
//! Nothing is debounced. One press can log several edges; that is the contact,
//! not the interrupt. A level wait would return immediately while the button
//! rests high, so this waits for edges only.
//!
//! `defmt` over RTT prints one line per edge. The port interrupts are claimed by
//! `hal::init` when the `embassy` feature is on, so there is nothing to bind.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::gpio::{Input, Pull};
use panic_halt as _;
use qingke_rt::entry;

#[embassy_executor::task]
async fn button(name: &'static str, mut pin: Input<'static>) -> ! {
    loop {
        pin.wait_for_falling_edge().await;
        defmt::info!("{=str} pressed", name);
        pin.wait_for_rising_edge().await;
        defmt::info!("{=str} released", name);
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let s1 = Input::new(p.PB22, Pull::Up);
    let s2 = Input::new(p.PB23, Pull::Up);
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
        spawner.spawn(button("S1", s1).unwrap());
        spawner.spawn(button("S2", s2).unwrap());
    })
}
