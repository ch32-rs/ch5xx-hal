# CH569 examples

`no_std` examples for `ch5xx-hal` on CH569 (QingKe V3A).

Run commands **from this directory**; `.cargo/config.toml` selects the RISC-V
target, `build-std`, and the probe runner.

```console
cd examples/ch569
cargo run --release --bin blinky
```

With more than one WCH-Link attached, select yours by serial:
`PROBE_RS_PROBE=1a86:8010:<SERIAL>` for `cargo run`, `--probe 1a86:8010:<SERIAL>`
for `probe-rs`. A bare `wlink status` picks the first link, which may be another
board.

## Memory map

This crate generates `memory.x` in `build.rs` instead of using the PAC's
`memory-x` output. The 128K window at `0x20020000` is split between writable
RAMX and the zero-wait ROM code area by `USER_MEM` in InfoFlash: 32K/64K/96K of
RAMX. Writes above the real end are dropped silently. The PAC's generated map
declares the 32K minimum, so a part with a larger option has to link its own map
to use the rest.

The default is 32K, the smallest option and the one every part can honour;
`build.rs` prints a warning when it is used. Link the size your part actually
has:

```console
CH569_USER_MEM=64 cargo build --release --bin blinky   # this EVT board
```

`RAMX` is also a named linker region. Place large buffers there with
`#[link_section = ".ramx"]`; the section is `NOLOAD`, so startup neither copies
nor clears it and the buffer must tolerate the previous contents.

## Features

`default = ["time-driver-systick"]`. The executor uses embassy's
`platform-spin`: the idle path of the default RISC-V platform calls `wfi` inside
a critical section, and on this core `wfi` never returns while `mstatus.MIE = 0`.

## Binaries

| Binary | Description |
| --- | --- |
| `blinky` | LED0/LED1/LED2 on PB24/PB22/PB23 |
| `uart_loopback` | UART0 internal loopback (`MCR.LOOP`), log over defmt |
| `embassy_time` | `Timer` on the SysTick time driver, log over defmt |
| `clk_hal` | `Sys::Pll(4)` 120 MHz through the HAL, plus a 1 s delay check |

**Never attach to a running core**: a debugger halt or memory read corrupts
the register file and the PC on this die. The bring-up diagnostics (atomics
and AMOs, memory and register probes, xip, misaligned access, the SysTick
crossing probe, executor fault logs, and the watchdog/PWMX/SPI-flash/reset
probes) staged their results in RAMX instead and were read once after a
measurement window; they are not shipped.
