//! Does the CH569 start-up clear `.bss` and copy `.data`?
//!
//! RAM on this die powers up with random content and survives a debugger
//! reset, so a mis-sized range in the linker script / start-up would leave
//! statics holding stale bytes. Reads every buffer before `hal::init` touches
//! anything.
//!
//! ```console
//! cargo build --release --no-default-features --bin init_probe
//! probe-rs read --chip CH569 --probe ... b32 0x20021000 32
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

#[no_mangle]
pub static mut BSS_BUF: [u32; 8] = [0; 8];

#[no_mangle]
pub static mut DATA_BUF: [u32; 8] = [
    0x1122_3344,
    0x5566_7788,
    0x99aa_bbcc,
    0xddee_ff00,
    0x0f0f_0f0f,
    0xf0f0_f0f0,
    0x1234_abcd,
    0xdead_beef,
];

#[no_mangle]
pub static mut BSS_BUF2: [u32; 8] = [0; 8];

extern "C" {
    static _sbss: u32;
    static _ebss: u32;
    static _edata: u32;
    static _data_vma: u32;
    static _data_lma: u32;
}

#[entry]
fn main() -> ! {
    // Read the buffers before anything else runs.
    let bss = core::ptr::addr_of!(BSS_BUF).cast::<u32>();
    let bss2 = core::ptr::addr_of!(BSS_BUF2).cast::<u32>();
    let data = core::ptr::addr_of!(DATA_BUF).cast::<u32>();

    scratch(0, 0x494e_4954); // "INIT"
    for i in 0..8 {
        scratch(1 + i, unsafe { core::ptr::read_volatile(bss.add(i)) });
    }
    for i in 0..8 {
        scratch(9 + i, unsafe { core::ptr::read_volatile(data.add(i)) });
    }
    for i in 0..8 {
        scratch(17 + i, unsafe { core::ptr::read_volatile(bss2.add(i)) });
    }

    unsafe {
        scratch(25, bss as u32);
        scratch(26, bss2 as u32);
        scratch(27, data as u32);
        scratch(28, core::ptr::addr_of!(_sbss) as u32);
        scratch(29, core::ptr::addr_of!(_ebss) as u32);
        scratch(30, core::ptr::addr_of!(_data_vma) as u32);
        scratch(31, core::ptr::addr_of!(_data_lma) as u32);
    }

    let _ = hal::init(Default::default());
    loop {
        core::hint::spin_loop();
    }
}
