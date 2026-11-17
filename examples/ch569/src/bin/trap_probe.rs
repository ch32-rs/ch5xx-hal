//! Trap-dump probe: a private `mtvec` handler that records the interrupted
//! registers, so the `delay_td_link` misaligned-store fault can be inspected.
//!
//! Same loop as `delay_td_link`; `main` points `mtvec` at `_trap_dump`, which
//! stores mcause/mepc/mtval and the GPRs at the fault, then spins. Results in
//! RAMX, read back with `probe-rs read` (wlink's memory reads return fill on
//! this part).
//!
//! ```console
//! cargo build --release --bin trap_probe
//! wlink -d 1a86:8010:7E238F0602BE --chip CH56X flash -e target/.../trap_probe
//! probe-rs read --chip CH569 --probe 1a86:8010:7E238F0602BE b32 0x20021000 16
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use ch5xx_hal::pac;
// Linked but never called: its presence is what triggers the fault.
use defmt_rtt as _;
use panic_halt as _;
use qingke::register::mtvec::{self, TrapMode};
use qingke_rt::entry;

const SCRATCH: *mut u32 = 0x2002_1000 as *mut u32;

core::arch::global_asm!(
    r#"
    .section .text
    .global _trap_dump
    .align 2
_trap_dump:
    la   t3, 0x20021000
    csrr t4, mcause
    sw   t4, 0x00(t3)
    csrr t4, mepc
    sw   t4, 0x04(t3)
    csrr t4, mtval
    sw   t4, 0x08(t3)
    sw   a0, 0x0c(t3)
    sw   ra, 0x10(t3)
    sw   sp, 0x14(t3)
    sw   a1, 0x18(t3)
    sw   a2, 0x1c(t3)
    sw   t0, 0x20(t3)
    sw   t1, 0x24(t3)
    sw   t2, 0x28(t3)
    sw   a3, 0x2c(t3)
    sw   a4, 0x30(t3)
    sw   a5, 0x34(t3)
    sw   a6, 0x38(t3)
    sw   a7, 0x3c(t3)
    sw   s0, 0x40(t3)
    sw   s1, 0x44(t3)
1:
    j 1b
"#
);

extern "C" {
    fn _trap_dump();
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());

    unsafe {
        mtvec::write(_trap_dump as *const () as usize, TrapMode::Direct);
        core::ptr::write_volatile(SCRATCH.add(8), hal::rcc::clocks().sysclk.0);
        core::ptr::write_volatile(SCRATCH.add(9), hal::delay::measured_sysclk().0);
    }

    let mut delay = hal::delay::Delay;
    let mut n: u32 = 0;
    loop {
        let start = pac::SYSTICK.cntl().read().cntl();
        delay.delay_ms(1000);
        let end = pac::SYSTICK.cntl().read().cntl();
        unsafe {
            core::ptr::write_volatile(SCRATCH.add(10), start);
            core::ptr::write_volatile(SCRATCH.add(11), end);
            core::ptr::write_volatile(SCRATCH.add(12), start.wrapping_sub(end));
            core::ptr::write_volatile(SCRATCH.add(13), n);
        }
        n = n.wrapping_add(1);
    }
}
