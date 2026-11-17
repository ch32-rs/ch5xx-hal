//! Drive the raw executor's `poll()` by hand, 1000 times.
//!
//! Isolates the run queue from the platform's `SIGNAL_WORK` / `wfi` loop: if
//! the task counters climb here, the queue and waker work and the thread-mode
//! idle path is what stalls the normal `Executor::run`.
//!
//! ```console
//! cargo build --release --no-default-features --bin poll_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 8
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

/// `Pending` once (after re-waking), then `Ready`: an await that never
/// completes suspends the task forever at that point.
struct Yield(bool);

impl core::future::Future for Yield {
    type Output = ();
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        if self.0 {
            core::task::Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    }
}

macro_rules! spinner {
    ($name:ident, $slot:expr) => {
        #[embassy_executor::task]
        async fn $name() {
            let mut n = 0u32;
            loop {
                n = n.wrapping_add(1);
                scratch($slot, n);
                Yield(false).await;
            }
        }
    };
}

spinner!(spinner_a, 1);
spinner!(spinner_b, 2);
spinner!(spinner_c, 3);

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x504f_4c4c); // "POLL"
    for i in 1..8 {
        scratch(i, 0);
    }

    static mut EXECUTOR: Option<embassy_executor::raw::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::raw::Executor::new(core::ptr::null_mut()))
    };

    let spawner = executor.spawner();
    spawner.spawn(spinner_a().unwrap());
    spawner.spawn(spinner_b().unwrap());
    spawner.spawn(spinner_c().unwrap());
    scratch(4, 1);

    // Head lives right after the `Option` discriminant of EXECUTOR.
    let head = || unsafe {
        let p = core::ptr::addr_of!(EXECUTOR).cast::<u32>();
        core::ptr::read_volatile(p.add(1))
    };

    for round in 0..1000u32 {
        if round < 6 {
            scratch(10 + round as usize * 2, head());
            unsafe { executor.poll() };
            scratch(11 + round as usize * 2, head());
        } else {
            unsafe { executor.poll() };
        }
        scratch(5, round);
    }

    scratch(6, 0xdead_0001);
    loop {
        core::hint::spin_loop();
    }
}
