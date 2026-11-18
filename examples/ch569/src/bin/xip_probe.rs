//! Run the `rq_strict_probe` round from flash and from RAMS.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use ch5xx_hal as hal;
use panic_halt as _;
use qingke::register::mtvec::{self, TrapMode};
use qingke_rt::entry;

core::arch::global_asm!(
    r#"
    .section .text
    .global _trap_dump
    .align 2
_trap_dump:
    csrr t0, mcause
    bgez t0, 2f
    andi t1, t0, 0xff
    slli t1, t1, 2
    lw   t2, 0(t1)
    jr   t2
2:
    la   t1, 0x20022000
    sw   t0, 0x00(t1)
    csrr t2, mepc
    sw   t2, 0x04(t1)
    csrr t2, mtval
    sw   t2, 0x08(t1)
    sw   sp, 0x0c(t1)
    sw   ra, 0x18(t1)
    sw   a0, 0x1c(t1)
    sw   a1, 0x20(t1)
    sw   a2, 0x24(t1)
    sw   a3, 0x28(t1)
    sw   a6, 0x34(t1)
    sw   s0, 0x58(t1)
    sw   s1, 0x5c(t1)
1:  j 1b
"#
);

extern "C" {
    fn _trap_dump();
}

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;
const SPAWNED: u32 = 1;
const QUEUED: u32 = 2;

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

#[inline(always)]
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

#[inline(always)]
fn round() -> u32 {
    wake(&N0);
    if HEAD.load(Ordering::Relaxed) != addr(&N0) {
        return 1;
    }
    if N0.next.load(Ordering::Relaxed) != 0 {
        return 2;
    }
    wake(&N1);
    if HEAD.load(Ordering::Relaxed) != addr(&N1) {
        return 3;
    }
    if N1.next.load(Ordering::Relaxed) != addr(&N0) {
        return 4;
    }
    wake(&N2);
    if HEAD.load(Ordering::Relaxed) != addr(&N2) {
        return 5;
    }
    if N2.next.load(Ordering::Relaxed) != addr(&N1) {
        return 6;
    }

    let mut p = HEAD.swap(0, Ordering::AcqRel);
    if p != addr(&N2) {
        return 7;
    }
    let mut seen = 0u32;
    while p != 0 {
        let node = match p {
            x if x == addr(&N0) => &N0,
            x if x == addr(&N1) => &N1,
            x if x == addr(&N2) => &N2,
            _ => return 8,
        };
        let next = node.next.load(Ordering::Relaxed);
        if next & 3 != 0 {
            return 9;
        }
        node.next.store(0, Ordering::Relaxed);
        node.state.fetch_and(!QUEUED, Ordering::AcqRel);
        let before = HEAD.load(Ordering::Relaxed);
        wake(node);
        if HEAD.load(Ordering::Relaxed) != p {
            return 10;
        }
        if node.next.load(Ordering::Relaxed) != before {
            return 11;
        }
        p = next;
        seen += 1;
        if seen > 3 {
            return 12;
        }
    }
    if seen != 3 {
        return 13;
    }
    if HEAD.swap(0, Ordering::AcqRel) != addr(&N0) {
        return 14;
    }

    for node in [&N0, &N1, &N2] {
        node.state.store(SPAWNED, Ordering::Relaxed);
        node.next.store(0, Ordering::Relaxed);
    }
    0
}

#[inline(never)]
fn run_flash(count: u32) -> u32 {
    for r in 0..count {
        let e = round();
        if e != 0 {
            return (r << 8) | e;
        }
    }
    0
}

#[link_section = ".data"]
#[inline(never)]
fn run_ram(count: u32) -> u32 {
    for r in 0..count {
        let e = round();
        if e != 0 {
            return (r << 8) | e;
        }
    }
    0
}

fn scratch(i: usize, v: u32) {
    unsafe { core::ptr::write_volatile(SCRATCH.add(i), v) };
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    scratch(0, u32::from_le_bytes(*b"XIPR"));
    scratch(1, 0);
    scratch(2, 0);
    scratch(6, run_flash as usize as u32);
    scratch(7, run_ram as usize as u32);
    unsafe { core::ptr::write_bytes(0x2002_2000 as *mut u32, 0, 32) };
    unsafe { mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct) };

    let e = run_flash(1_000_000);
    scratch(1, if e == 0 { 0xa5a5_0001 } else { e });

    let e = run_ram(1_000_000);
    scratch(2, if e == 0 { 0xa5a5_0002 } else { e });

    loop {
        core::hint::spin_loop();
    }
}
