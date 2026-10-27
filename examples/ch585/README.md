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
| `gpio_in` | reads the user buttons S1 (PB22) and S2 (PB23) with pull-up and logs each level change |
| `defmt_rtt` | logging over SEGGER RTT |
| `cpuid` | the core's identity CSRs: `misa`, `mvendorid`, `marchid`, `mimpid`, `mhartid` |
| `bitmanip` | whether the `B` that `misa` reports is the ratified `Zb*` encoding, one instruction at a time |
| `uart_loopback` | blocking UART0 with `MCR.LOOP`, so the byte path is checked without external wiring |
| `uart_echo` | async (embassy) UART3 on PA4/PA5 at 115200 8N1 with `Fsys` at 78 MHz (HSE PLL), echoing an external device (header P2 pins 1 and 2) and mirroring each received line to `defmt` over RTT |
| `clk_probe` | the firmware's view of the clock registers, nominal clock and measured SysTick rate |
| `clk_set` | boot from `HSI_PLL_24MHZ` and report the register readback and both rates |
| `embassy_time` | two tasks on an embassy executor driven by the SysTick time driver |
| `embassy_time_tmr` | the same on a TMR driver, so `delay` keeps the raw SysTick counter |

## Async UART

`uart_echo` uses the interrupt-driven halves: `Uart::new` takes an interrupt
binding created by `hal::bind_interrupts!`, and `write`/`flush`/`read` are
`async`. There is no DMA — the driver moves bytes through the eight-byte FIFO
and wakes from the instance's interrupt.

The example runs **115200 8N1** with `Fsys` from `rcc::Config::HSE_PLL_78MHZ`
(78 MHz off the 32 MHz crystal). The baud is `Fsys / (8 * DIV * DL)`, with `DIV`
in 1..=127 and `DL` a 16-bit integer, so 115200 becomes `DL = 85` = 114706 baud
(-0.43%), inside the datasheet's 0.5% transmit budget. The reset clock
(5.333 MHz) cannot generate 115200 (`DL = 6` is 111.1 kbaud, -3.5%), which is
why the clock is raised first. `uart_loopback` keeps the 115200 default because
both directions share the divider, so the error cancels.

## Clock configuration

`hal::init` takes an `rcc::Config` and applies it before any driver starts.
`Config::default()` is the datasheet reset value, HSI 16 MHz / 3 = 5.333 MHz, so
programs that do not configure a clock behave as before. Presets mirror WCH's
C SDK `CLK_SOURCE_*` values:

```rust
let p = hal::init(Default::default());                 // 5.333 MHz
let p = hal::init(rcc::Config::HSI_PLL_24MHZ.into());  // 24 MHz, HSI PLL, no crystal
let p = hal::init(rcc::Config::HSE_PLL_78MHZ.into());  // 78 MHz, needs the crystal
let p = hal::init(rcc::Config::hsi_nearest(hal::time::hz(30_000_000)).into()); // 31.2 MHz
```

`HSE_*` presets require the board's 32 MHz crystal; the CH585M-R1-1v1 fits one
and the presets select its 18 pF load capacitance. `rcc::clocks().sysclk` is the
nominal frequency of the configured tree; `delay::measured_sysclk()` is the
RTC-calibrated rate `delay` uses for busy-waits.

`rcc::set_config` can change the clock at runtime, but only the tree and the
`clocks()` cache: SysTick/timebase/UART dividers stay on the old frequency.
`delay::recalibrate` re-measures the busy-wait rate where the SysTick time
driver is not selected.

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

## Target features

`.cargo/config.toml` passes two `target-feature`s, and they are not the same kind
of thing:

- **`+b`** is standard and stable. The V3C implements the ratified
  `Zba`/`Zbb`/`Zbs` encodings — `bitmanip` executes each instruction on the part
  and they all run; there is no trap. Lighting up `+b` shrinks `blinky` by about
  16 KiB because LLVM stops expanding `clz`/`cpop`/`rev8` into loops. Turning it
  on is safe.
- **`+xwchc`** is WCH's `XW` extension: compressed byte/halfword loads and stores
  (`c.lbu`, `c.lhusp`, `c.sbsp`, …; manual table 1-1). Rust does not model it, so
  rustc does not know the name and warns on every build — "unknown and unstable
  feature … `xwchc`", slated to become a hard error
  ([rust-lang/rust#162235]). It is passed anyway because the backend honours it.
  Note `+zcb` is *not* an alternative: LLVM rejects the pair as incompatible,
  because `XW` predates Zcb and both claim the same compressed encodings.

So the answer to "is `B` enabled like `+xwchc`?" is no: `B` is a ratified
standard feature the compiler understands, `xwchc` is a vendor extension it does
not. The only thing they share is the `-C target-feature=` syntax.

`misa` reporting `B` was not enough on its own to justify `+b`: the manual calls
bit 1 "tentatively reserved", and the pre-ratification `B` draft uses different
encodings from the ratified `Zb*` set. `bitmanip` resolves it by execution, with
three negative controls (`amoadd.w`, `amoswap.w`, `unimp`) that must trap — if
they ever stop trapping, its `OK` lines are meaningless.

[rust-lang/rust#162235]: https://github.com/rust-lang/rust/issues/162235
