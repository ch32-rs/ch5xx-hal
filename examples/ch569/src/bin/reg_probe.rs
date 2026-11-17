//! Parks the core with known values in a0/a1/t3, to check whether `wlink regs`
//! reads the GPR file truthfully (`x10` was suspect on CH569).
//!
//! ```console
//! cargo build --release --bin reg_probe
//! wlink -d ... --chip CH56X flash -e target/.../reg_probe
//! wlink -d ... regs
//! ```

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use panic_halt as _;
use qingke_rt::entry;

core::arch::global_asm!(
    r#"
    .section .text
    .global _reg_probe
    .align 2
_reg_probe:
    li a0, 0xCAFEBABE
    li a1, 0x13572468
    li a2, 0x0BADF00D
    li t3, 0x11112222
1:
    addi t3, t3, 0
    j 1b
"#
);

extern "C" {
    fn _reg_probe() -> !;
}

#[entry]
fn main() -> ! {
    let _ = hal::init(Default::default());
    unsafe { _reg_probe() }
}
