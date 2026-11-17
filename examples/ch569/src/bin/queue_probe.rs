//! Replica of embassy's run queue: `fetch_or(state)`, then CAS a linked head.
//!
//! Three nodes, the same instruction shapes the executor uses, 100k rounds.
//! Records the first round whose list is short or whose plain load of the head
//! disagrees with the atomic swap.
//!
//! ```console
//! cargo build --release --no-default-features --bin queue_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 10
//! ```

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

#[repr(C)]
struct Node {
    state: AtomicU32,
    next: AtomicUsize,
}

static HEAD: AtomicUsize = AtomicUsize::new(0);
static N0: Node = Node { state: AtomicU32::new(0), next: AtomicUsize::new(0) };
static N1: Node = Node { state: AtomicU32::new(0), next: AtomicUsize::new(0) };
static N2: Node = Node { state: AtomicU32::new(0), next: AtomicUsize::new(0) };

fn push(node: &'static Node) {
    let prev = node.state.fetch_or(2, Ordering::AcqRel);
    if prev & 2 == 0 {
        let me = node as *const Node as usize;
        let mut head = HEAD.load(Ordering::Relaxed);
        loop {
            node.next.store(head, Ordering::Relaxed);
            match HEAD.compare_exchange_weak(head, me, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(actual) => head = actual,
            }
        }
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x5155_4555); // "QUEU"
    for i in 1..10 {
        scratch(i, 0);
    }

    let mut plain_null = 0u32;
    let mut short = 0u32;

    for round in 0..100_000u32 {
        for n in [&N0, &N1, &N2] {
            n.state.store(0, Ordering::Relaxed);
            n.next.store(0, Ordering::Relaxed);
        }
        HEAD.store(0, Ordering::Relaxed);

        push(&N0);
        push(&N1);
        push(&N2);

        if HEAD.load(Ordering::Relaxed) == 0 {
            plain_null = plain_null.wrapping_add(1);
            if plain_null == 1 {
                scratch(4, round);
            }
        }

        let mut p = HEAD.swap(0, Ordering::AcqRel);
        let mut seen = 0u32;
        let mut bits = 0u32;
        while p != 0 {
            let idx = if p == &N0 as *const Node as usize {
                0
            } else if p == &N1 as *const Node as usize {
                1
            } else if p == &N2 as *const Node as usize {
                2
            } else {
                bits |= 0x100;
                break;
            };
            bits |= 1 << idx;
            seen += 1;
            if seen > 8 {
                break;
            }
            let node = [&N0, &N1, &N2][idx];
            node.state.fetch_and(!2, Ordering::AcqRel);
            p = node.next.load(Ordering::Relaxed);
        }

        if seen != 3 || bits != 7 {
            short = short.wrapping_add(1);
            if short == 1 {
                scratch(5, round);
                scratch(6, seen);
                scratch(7, bits);
            }
        }

        if short != 0 || plain_null != 0 {
            scratch(1, round);
            scratch(2, plain_null);
            scratch(3, short);
            break;
        }
    }

    scratch(8, 0xdead_0001);
    loop {
        core::hint::spin_loop();
    }
}
