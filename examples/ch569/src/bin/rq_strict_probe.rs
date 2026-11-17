//! Deterministic replay of the executor's run queue with a readback after
//! every mutation.
//!
//! Single-threaded, no interrupts (`--no-default-features`), so any failing
//! check is a wrong value from a primitive, not a race. The first failure
//! records `(round, check, expected, actual)` at 0x20021004..0x20021010 and
//! 0xdead0000 at 0x20021014.
//!
//! Checks: 1 head after wake, 2 `next` after wake, 3 popped pointer,
//! 4 `next` chased during the drain.
//!
//! ```console
//! cargo build --release --no-default-features --bin rq_strict_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 8
//! ```

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const RING: *mut u32 = 0x2002_1300 as *mut u32;
const SPAWNED: u32 = 1;
const QUEUED: u32 = 2;

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

/// `seq, round, head, n0.next, n1.next, n2.next` per slot, 16 slots.
fn trace(seq: u32, round: u32) {
    let slot = (seq & 15) as usize * 6;
    unsafe {
        core::ptr::write_volatile(RING.add(slot), seq);
        core::ptr::write_volatile(RING.add(slot + 1), round);
        core::ptr::write_volatile(RING.add(slot + 2), HEAD.load(Ordering::Relaxed) as u32);
        core::ptr::write_volatile(RING.add(slot + 3), N0.next.load(Ordering::Relaxed) as u32);
        core::ptr::write_volatile(RING.add(slot + 4), N1.next.load(Ordering::Relaxed) as u32);
        core::ptr::write_volatile(RING.add(slot + 5), N2.next.load(Ordering::Relaxed) as u32);
    }
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

fn addr(node: &'static Node) -> usize {
    node as *const Node as usize
}

fn expect(round: u32, check: u32, expected: usize, actual: usize) {
    if expected == actual {
        return;
    }
    scratch(1, round);
    scratch(2, check);
    scratch(3, expected as u32);
    scratch(4, actual as u32);
    scratch(5, 0xdead_0000);
    loop {
        core::hint::spin_loop();
    }
}

/// `State::run_enqueue` + the intrusive push, exactly as the executor does it.
fn wake(node: &'static Node) {
    if node.state.fetch_or(QUEUED, Ordering::AcqRel) & QUEUED != 0 {
        return;
    }
    let me = addr(node);
    let mut prev = HEAD.load(Ordering::Relaxed);
    loop {
        node.next.store(prev, Ordering::Relaxed);
        match HEAD.compare_exchange_weak(prev, me, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(actual) => prev = actual,
        }
    }
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, 0x5251_5354); // "RQST"
    for i in 1..8 {
        scratch(i, 0);
    }
    unsafe {
        for i in 0..(16 * 6) {
            core::ptr::write_volatile(RING.add(i), 0);
        }
    }

    for round in 0..u32::MAX {
        trace(round, 0);
        wake(&N0);
        expect(round, 1, addr(&N0), HEAD.load(Ordering::Relaxed));
        expect(round, 2, 0, N0.next.load(Ordering::Relaxed));
        wake(&N1);
        expect(round, 1, addr(&N1), HEAD.load(Ordering::Relaxed));
        expect(round, 2, addr(&N0), N1.next.load(Ordering::Relaxed));
        wake(&N2);
        expect(round, 1, addr(&N2), HEAD.load(Ordering::Relaxed));
        expect(round, 2, addr(&N1), N2.next.load(Ordering::Relaxed));

        let mut p = HEAD.swap(0, Ordering::AcqRel);
        expect(round, 3, addr(&N2), p);
        let mut seen = 0usize;
        while p != 0 {
            let node = match p {
                x if x == addr(&N0) => &N0,
                x if x == addr(&N1) => &N1,
                x if x == addr(&N2) => &N2,
                _ => {
                    scratch(1, round);
                    scratch(2, 3);
                    scratch(3, seen as u32);
                    scratch(4, p as u32);
                    scratch(5, 0xdead_0001);
                    loop {
                        core::hint::spin_loop();
                    }
                }
            };
            let next = node.next.load(Ordering::Relaxed);
            expect(round, 4, 0, next & 3);
            node.next.store(0, Ordering::Relaxed);
            node.state.fetch_and(!QUEUED, Ordering::AcqRel);
            // The polled future wakes itself.
            let before = HEAD.load(Ordering::Relaxed);
            wake(node);
            expect(round, 1, p, HEAD.load(Ordering::Relaxed));
            expect(round, 2, before, node.next.load(Ordering::Relaxed));
            p = next;
            seen += 1;
            if seen > 3 {
                break;
            }
        }
        expect(round, 3, 3, seen);
        // The three self-wakes rebuilt the list in reverse drain order.
        expect(round, 3, addr(&N0), HEAD.swap(0, Ordering::AcqRel));

        for node in [&N0, &N1, &N2] {
            node.state.store(SPAWNED, Ordering::Relaxed);
            node.next.store(0, Ordering::Relaxed);
        }
        scratch(6, round);
    }

    scratch(7, 0xdead_0002);
    loop {
        core::hint::spin_loop();
    }
}
