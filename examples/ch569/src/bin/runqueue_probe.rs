//! Replay the executor's run queue: `fetch_or(state)` + CAS push, `swap(0)`
//! dequeue, and a self-wake for every node polled.
//!
//! Any round whose popped list misses a node, or leaves a node marked queued
//! but unreachable, is recorded.
//!
//! ```console
//! cargo build --release --no-default-features --bin runqueue_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 10
//! ```

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const SPAWNED: u32 = 1;
const QUEUED: u32 = 2;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

#[repr(C)]
struct Node {
    state: AtomicU32,
    next: AtomicUsize,
}

static HEAD: AtomicUsize = AtomicUsize::new(0);
static N0: Node = Node { state: AtomicU32::new(SPAWNED), next: AtomicUsize::new(0) };
static N1: Node = Node { state: AtomicU32::new(SPAWNED), next: AtomicUsize::new(0) };
static N2: Node = Node { state: AtomicU32::new(SPAWNED), next: AtomicUsize::new(0) };

/// `State::run_enqueue` + the intrusive push.
fn wake(node: &'static Node) {
    if node.state.fetch_or(QUEUED, Ordering::AcqRel) & QUEUED != 0 {
        return;
    }
    let me = node as *const Node as usize;
    let mut prev = HEAD.load(Ordering::Relaxed);
    loop {
        node.next.store(prev, Ordering::Relaxed);
        match HEAD.compare_exchange_weak(prev, me, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(actual) => prev = actual,
        }
    }
}

fn index(p: usize) -> Option<usize> {
    if p == &N0 as *const Node as usize {
        Some(0)
    } else if p == &N1 as *const Node as usize {
        Some(1)
    } else if p == &N2 as *const Node as usize {
        Some(2)
    } else {
        None
    }
}

fn reachable() -> u32 {
    let mut bits = 0u32;
    let mut p = HEAD.load(Ordering::Relaxed);
    let mut guard = 0;
    while p != 0 && guard < 8 {
        guard += 1;
        let Some(i) = index(p) else { return 0xffff_ffff };
        bits |= 1 << i;
        p = [&N0, &N1, &N2][i].next.load(Ordering::Relaxed);
    }
    bits | guard << 8
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x5251_5052); // "RQPR"
    for i in 1..10 {
        scratch(i, 0);
    }

    for round in 0..2_000_000u32 {
        // Queue all three.
        wake(&N0);
        wake(&N1);
        wake(&N2);

        // The executor's dequeue: take the whole list, then poll each node.
        let mut p = HEAD.swap(0, Ordering::AcqRel);
        let mut seen = 0u32;
        let mut bad_p = 0u32;
        while p != 0 && seen < 16 {
            let Some(i) = index(p) else {
                bad_p = p as u32;
                break;
            };
            let node = [&N0, &N1, &N2][i];
            let next = node.next.load(Ordering::Relaxed);
            node.next.store(0, Ordering::Relaxed);
            node.state.fetch_and(!QUEUED, Ordering::AcqRel);
            seen += 1;
            // The future wakes itself while being polled.
            wake(node);
            p = next;
        }

        if bad_p != 0 {
            scratch(1, round);
            scratch(2, bad_p);
            scratch(3, seen);
            scratch(4, 0xdead_0000);
            break;
        }

        // Every polled node must be back on the queue, and only those.
        let back = reachable();
        if seen != 3 || back & 7 != 7 || back >> 8 != 3 {
            scratch(1, round);
            scratch(2, seen);
            scratch(3, back);
            break;
        }

        // Round-trip: draining again must find the same three.
        let mut p = HEAD.swap(0, Ordering::AcqRel);
        let mut bits = 0u32;
        let mut n = 0u32;
        while p != 0 && n < 8 {
            n += 1;
            let Some(i) = index(p) else { bits = 0xffff_ffff; break };
            bits |= 1 << i;
            let node = [&N0, &N1, &N2][i];
            p = node.next.load(Ordering::Relaxed);
        }
        for node in [&N0, &N1, &N2] {
            node.state.store(SPAWNED, Ordering::Relaxed);
            node.next.store(0, Ordering::Relaxed);
        }
        if bits != 7 || n != 3 {
            scratch(6, round);
            scratch(7, bits);
            scratch(8, n);
            break;
        }
        scratch(9, round);
    }

    scratch(5, 0xdead_0001);
    loop {
        core::hint::spin_loop();
    }
}
