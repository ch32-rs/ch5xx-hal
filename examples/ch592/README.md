# CH592 examples

Examples for the CH59x (CH592X) family. The board is an official CH59x EVT.

```console
cd examples/ch592
cargo build --release --bin hello
cargo run --release --bin hello             # probe-rs: flash + RTT
```

`cargo run` uses `probe-rs run --chip CH592`, which flashes, resets and streams
`defmt` over RTT in one step. When several WCH-Links are attached, address the
one in front of the board by serial:

```console
PROBE_RS_PROBE=1a86:8010:<serial> cargo run --release --bin hello
```

Without the variable, probe-rs picks the first link; verify the chip is CH592
before flashing.

## Binaries

| Binary | Shows |
| --- | --- |
| `hello` | Default 6.4 MHz clock, `delay`, defmt logging |
| `clk_pll48` | 48 MHz from the 480 MHz PLL, Flash timing and read-back |
| `embassy_time` | Embassy time driver; pick SysTick or TMR0–TMR3 with features |
| `uart_loopback` | UART0 internal loopback, self-checked |

Self-checking examples log a `SELFTEST PASS` or `SELFTEST FAIL` line, following
the CH585 bench tests: drive both ends inside the chip or with one jumper, then
let the firmware compare what came back.

Build for a TMR timebase without editing the manifest:

```console
cargo build --release --bin embassy_time --no-default-features --features time-driver-tmr1
```

All four TMR instances work, including TMR0/TMR3, whose register block omits the
timer-local DMA registers.

## Notes

- `probe-rs` now attaches to this die; older versions failed with a bulk-read
  timeout while the stock firmware held the debug pins.
- The HAL also implements the WCH-Link SDI print channel on CH592
  (`hal::debug::SDIPrint`), but the examples use defmt/RTT so logs are captured
  by the same `cargo run` that flashes.
- A `+b` target feature traps on this die; the crate builds with no extra
  target features.
