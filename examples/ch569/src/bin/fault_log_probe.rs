//! `rq_strict_probe` under a resumable trap handler.
//!
//! Every fault is logged to a 16-slot ring (ten words each) at 0x20023000 and
//! the faulting instruction is skipped, so a run collects statistics instead of
//! stopping at the first event. The count is kept in the ring's first word.
//!
//! Slot layout: mcause, mepc, mtval, sp, a0, a1, a2, a3, a6, t6.
//!
//! ```console
//! cargo build --release --no-default-features --bin fault_log_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20023000 40
//! ```

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke::register::mtvec::{self, TrapMode};
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const LOG: *mut u32 = 0x2002_3000 as *mut u32;
const SPAWNED: u32 = 1;
const QUEUED: u32 = 2;

core::arch::global_asm!(
    r#"
    .section .text
    .global _trap_log
    .align 2
_trap_log:
    csrr t0, mcause
    bltz t0, _trap_log_irq
    /* exception: log and skip */
    la   t1, 0x20023000
    lw   t2, 0(t1)
    addi t3, t2, 1
    sw   t3, 0(t1)
    andi t3, t2, 15
    li   t4, 40
    mul  t3, t3, t4
    add  t3, t1, t3
    sw   t0, 4(t3)
    csrr t4, mepc
    sw   t4, 8(t3)
    csrr t4, mtval
    sw   t4, 12(t3)
    sw   sp, 16(t3)
    sw   a0, 20(t3)
    sw   a1, 24(t3)
    sw   a2, 28(t3)
    sw   a3, 32(t3)
    sw   a6, 36(t3)
    /* skip the faulting instruction */
    csrr t4, mepc
    addi t4, t4, 4
    csrw mepc, t4
    mret

_trap_log_irq:
    andi t1, t0, 0xff
    slli t1, t1, 2
    lw   t2, 0(t1)
    jr   t2
"#
);

extern "C" {
    fn _trap_log();
}

static HEAD: AtomicUsize = AtomicUsize::new(0);
static N0: Node = Node { state: AtomicU32::new(SPAWNED), next: AtomicUsize::new(0) };
static N1: Node = Node { state: AtomicU32::new(SPAWNED), next: AtomicUsize::new(0) };
static N2: Node = Node { state: AtomicU32::new(SPAWNED), next: AtomicUsize::new(0) };

#[repr(C)]
struct Node {
    state: AtomicU32,
    next: AtomicUsize,
}

fn addr(node: &'static Node) -> usize {
    node as *const Node as usize
}

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

fn expect(round: u32, check: u32, expected: usize, actual: usize) {
    if expected == actual {
        return;
    }
    scratch(1, round);
    scratch(2, check);
    scratch(3, expected as u32);
    scratch(4, actual as u32);
    scratch(5, 0xdead_0000 | check);
    loop {
        core::hint::spin_loop();
    }
}

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

    scratch(0, u32::from_le_bytes(*b"FLGP"));
    for i in 1..8 {
        scratch(i, 0);
    }
    // Blank CodeFlash reads back a per-chip sentinel, not 0xFFFFFFFF.
    scratch(10, unsafe { core::ptr::read_volatile(0x0000_2000 as *const u32) });
    scratch(11, unsafe { core::ptr::read_volatile(0x0000_8000 as *const u32) });
    scratch(12, unsafe { core::ptr::read_volatile(0x0008_2000 as *const u32) });
    unsafe {
        core::ptr::write_bytes(LOG, 0, 1 + 16 * 10);
        mtvec::write(_trap_log as *const () as usize, TrapMode::Direct);
    }

    for round in 0..u32::MAX {
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
