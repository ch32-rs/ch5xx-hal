# CH569 examples

`no_std` binaries and bring-up probes for `ch5xx-hal` on CH569 (QingKe V3A).

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

This crate ships its own `memory.x` instead of the PAC's `memory-x` output. The
128K window at `0x20020000` is split between writable RAMX and the zero-wait ROM
code area by `USER_MEM` in InfoFlash: 32K/64K/96K of RAMX. This board has the
64K + 64K split and the PAC declares 96K, so a map taken from the PAC can place
data where writes are silently dropped. If your board's `USER_MEM` differs,
change `LENGTH` of `RAMX` in `memory.x`.

## Features

`default = ["time-driver-systick"]`. Bins that drive the SysTick vector
themselves own it, so they are built without the time driver:

```console
cargo build --release --no-default-features --features raw-systick
```

The executor uses embassy's `platform-spin`: the idle path of the default RISC-V
platform calls `wfi` inside a critical section, and on this core `wfi` never
returns while `mstatus.MIE = 0`.

## Collecting results

Probes stage their results in RAMX from `0x20021000` and park. **Never attach to
a running core**: a debugger halt or memory read corrupts the register file and
the PC on this die. Flash, leave the chip alone for the measurement window, then
read once:

```console
wlink -d 1a86:8010:<SERIAL> flash -e target/riscv32imac-unknown-none-elf/release/<bin>
sleep 60
probe-rs read --chip CH569 --probe 1a86:8010:<SERIAL> b32 0x20021000 8
```

## Binaries

| Binary | Description |
| --- | --- |
| `blinky` | LED0/LED1/LED2 on PB24/PB22/PB23 |
| `gpio_in_probe` | Input path and both internal pulls on eight pins; no wiring |
| `gpio_pull` | Pull settings on the LED pins |
| `uart_loop_probe` | UART0 internal loopback and UART1/UART3 transmit completion |
| `crossing_probe` | SysTick down-counter reload behaviour at the zero crossing |
| `tmr_probe` | TMR0/1/2 counter rate against `Instant` |
| `tmr_irq_probe` | TMR0/1/2 period interrupt through PFIC and the HAL handler |
| `timer_probe` | `embassy-time` timer ticks off the SysTick driver |
| `yield_raw` | Three always-ready executor tasks; run unattended, then read |
| `wdg_probe` | Watchdog reset: cause flag, `GLOB_RESET_KEEP`, control register |
| `swreset_probe` | `watchdog::software_reset` cause flag and reset keep |
| `ramx_map_probe` | Writable RAMX window and the InfoFlash option words |

The remaining binaries are silicon and driver diagnostics used during bring-up:
atomics and AMOs, memory and register torture, xip, misaligned access, the
SysTick probes, and the executor fault logs. They stage the same way.
