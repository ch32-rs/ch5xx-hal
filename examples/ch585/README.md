# CH585 examples

Example binaries for `ch5xx-hal` on the CH585 (QingKe V3C). Each one is a
`no_std` binary run straight on the part; none of them is part of the HAL crate.

Everything here runs **from this directory** — the target triple, `build-std` and
the runner live in `.cargo/config.toml`. Building from the repository root fails
with `invalid Mach-O section specifier` because no RISC-V target is selected
there.

```console
cd examples/ch585
cargo run --release --bin blinky
```

The runner is `probe-rs run --chip CH585`, so `cargo run` flashes, resets and
streams `defmt` over RTT in one step. `.cargo/config.toml` keeps a `wlink flash`
runner commented out for when you only want to program the part.

## Binaries

| Binary | Shows |
| --- | --- |
| `blinky` | GPIO output and the blocking `delay`; LED0 (PB16) and LED1 (PB17) in opposite phase |
| `defmt_rtt` | logging over SEGGER RTT |
| `cpuid` | the core's identity CSRs: `misa`, `mvendorid`, `marchid`, `mimpid`, `mhartid` |
| `clk_probe` | the firmware's view of `SYS.CLK_CFG` and the measured SysTick rate |
| `embassy_time` | two tasks on an embassy executor driven by the SysTick time driver |
| `embassy_time_tmr` | the same on a TMR driver, so `delay` keeps the raw SysTick counter |

## Time driver features

`embassy_time` and `embassy_time_tmr` need a time driver, selected with the
features this crate forwards from the HAL. The default is
`time-driver-systick`; switch without editing `Cargo.toml`:

```console
cargo run --release --bin embassy_time_tmr --no-default-features --features time-driver-tmr0
```

`embassy_time` works with any of them. `embassy_time_tmr` needs one of
`time-driver-tmr0` … `time-driver-tmr3` — its point is that a TMR leaves SysTick
free, so it refuses to build with the SysTick driver.

The non-embassy examples do not care which driver is selected; `delay` works
either way.

## Console output

CH5xx has no SDI printf (the debug-module `DATA0`/`DATA1` handshake is not
present). Use `defmt` over SEGGER RTT — no special chip support is needed and
`probe-rs` handles it natively.

`DEFMT_LOG` controls the log level at compile time and defaults to `error`,
which silently discards `info!`. `.cargo/config.toml` sets it to `info` here.

To flash and attach separately:

```console
probe-rs download --chip CH585 target/riscv32imc-unknown-none-elf/release/defmt_rtt
probe-rs attach --chip CH585 target/riscv32imc-unknown-none-elf/release/defmt_rtt
```

## Observing with a debugger

Every `wlink`/`probe-rs` memory read **halts the core and does not resume it**,
so polling a live counter freezes the firmware and makes working code look hung;
repeatedly sampling a GPIO keeps catching the same phase and makes alternating
LEDs look stuck.

Verify by recording a *history* into RAM and reading it once at the end (see the
`BLINKS` counter in `blinky`), or flash, wait untouched, then take a single read.
Prefer `probe-rs read` over `wlink dump` for memory — `wlink dump` can return
all-`0xFF` where data exists.

## Target and atomics

These build for `riscv32imc-unknown-none-elf`. The QingKe V3C is RV32IMBC — there
is **no A extension**, and `amoadd.w`/`amoswap.w` raise a hardware exception on
real silicon. `misa` reports this directly: `cargo run --release --bin cpuid`
prints `0x40901106`, i.e. RV32 with `BCIMUX` and no `A`.

That is why `portable-atomic` with the `critical-section` feature is a dependency
here as well as in the HAL.
