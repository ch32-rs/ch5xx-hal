# ch5xx-hal

Rust HAL (Hardware Abstraction Layer) crate for WCH's CH5xx BLE microcontrollers.

> **Note**
> This project is under development. Only a small part of the peripheral set is
> implemented, and only CH585 has been exercised on real hardware. Expect the
> API to change.

This HAL follows the [ch32-hal](https://github.com/ch32-rs/ch32-hal) layout and
the metapac approach: register access comes from a single
[`ch5xx-metapac`](https://github.com/ch32-rs/ch5xx-data) crate, so the same HAL
code serves every supported chip in the family.

## Supported devices

| Chip  | Core      | Status                          |
| ----- | --------- | ------------------------------- |
| CH585 | QingKe V3C | Bring-up target; blinky verified on hardware |

Chip support is selected with a Cargo feature (`ch585`). Exactly one must be
enabled.

## Implemented so far

- `init()` — clock/frequency setup and GPIO bring-up
- `gpio` — input, output, flex pins, internal pull up/down, drive strength
- `delay` — `embedded-hal` `DelayNs`, backed by the core SysTick (or by the
  time driver's timebase under `time-driver-systick`)
- `rcc` — reports the measured system clock

Everything else (UART, SPI, I2C, timers, BLE, USB, …) is not implemented yet.
There is also no clock-tree configuration yet — the clock the boot ROM leaves
in place is used as-is. See [System clock](#system-clock).

## What differs from CH32

The CH5xx register model is not a CH32 one, so a few things look different:

- **No RCC.** Clocks, power and the pin mux live in `SYS` / `System_control` at
  `0x4000_1000`.
- **No AFIO.** Peripheral signals are routed with the `SYS.PIN_ALTERNATE`
  "alternate pin enable" bits rather than a remap register.
- **One GPIO block** with per-port register groups (`pa_out`, `pb_dir`, …)
  instead of one peripheral per port.

## System clock

On the CH585 used for bring-up, the firmware reads `CLK_CFG = 0x0003` — the
datasheet's reset value: `CLK_SYS_MOD = 00`, `CLK_PLL_DIV = 3`, `OSC32M_SEL = 0`,
i.e. **HSI 16 MHz / 3 = 5.333 MHz**. The boot ROM powers the PLL but does not
route the system clock through it.

> **Do not read clock registers with `probe-rs read`.** It halts the core, and a
> 16-bit read of `0x4000_1008` returns `0x014D` (mixing in the neighbouring
> `HFCK_PWR_CTRL` byte) rather than the `0x0003` the firmware sees. Read them
> from firmware and print over RTT: `cargo run --release --bin clk_probe`.

`delay::init` measures SysTick against the RTC's 32 kHz counter at startup (the
same idea as WCH's `OSC_CAL`) and publishes the result through `rcc::clocks()`.
That measurement is a calibration, not a constant — it varies by a few hundred
Hz between resets because the internal 32 kHz RC is the reference. The window is
4096 RTC counts and the median of five rounds is used: a shorter window proved
too sensitive to the skew between the two counter reads, reporting 5.33–7.43 MHz
for the same silicon (see "Embassy time driver" below). Runtime reconfiguration
of the system clock (HSI/HSE presets, PLL) is not implemented; it needs the SAM
handshake and Flash access timing.

## Example

`examples/ch585` blinks LED0 (PB16) and LED1 (PB17) in opposite phase:

```console
cd examples/ch585
cargo run --release --bin blinky
```

| Binary | Shows |
| --- | --- |
| `blinky` | GPIO output and the blocking `delay` |
| `defmt_rtt` | logging over SEGGER RTT |
| `clk_probe` | the firmware's view of `SYS.CLK_CFG` and the measured SysTick rate |
| `embassy_time` | two tasks on an embassy executor driven by the SysTick time driver |
| `embassy_time_tmr` | the same on a TMR driver, so `delay` keeps the raw SysTick counter |

The runner is `probe-rs run --chip CH585`, so `cargo run` flashes, resets, and
streams `defmt` output in one step. A `wlink flash` runner is kept commented out
in `.cargo/config.toml` if you only want to program the part.

> **Careful when observing with a debugger:** every `wlink`/`probe-rs` memory
> read halts the core and does not resume it, so polling a live counter will
> freeze the firmware. Read state once after waiting, or record a history into
> RAM and read it afterwards.

## Console output (defmt + RTT)

CH5xx has no SDI printf (the debug-module `DATA0`/`DATA1` handshake is not
present). Use `defmt` over SEGGER RTT instead — no special chip support is
needed, and `probe-rs` handles it natively:

```console
cd examples/ch585
cargo run --release --bin defmt_rtt
```

This uses the `probe-rs run --chip CH585` runner from `.cargo/config.toml`. If
you want to flash and attach separately instead:

```console
probe-rs download --chip CH585 target/riscv32imc-unknown-none-elf/release/defmt_rtt
probe-rs attach --chip CH585 target/riscv32imc-unknown-none-elf/release/defmt_rtt
```

Add `defmt` and `defmt-rtt` to your binary's dependencies. The `DEFMT_LOG` env
variable controls log level at compile time (default is `error`; `.cargo/config.toml`
sets `info` for the examples).

## Target: `riscv32imc`, not `riscv32imac`

The QingKe V3C core is **RV32IMBC — there is no A extension**, and executing
`amoadd.w` / `amoswap.w` raises a hardware exception on real CH585 silicon.
Building for the usual `riscv32imac-unknown-none-elf` therefore produces a
binary that faults the first time any atomic read-modify-write runs.

The examples build for `riscv32imc-unknown-none-elf`, and every atomic in this
crate goes through `portable-atomic` with the `critical-section` feature, which
supplies `fetch_add` / `compare_exchange` from a critical section. Enable the
same feature in any binary crate that shares state with the HAL.

## Embassy time driver

`ch5xx-hal` has an `embassy-time-driver` implementation, selected by a feature:

| Feature | Counter used | Cost |
| --- | --- | --- |
| `time-driver-systick` | core SysTick | takes SysTick from `delay` |
| `time-driver-tmr0` … `time-driver-tmr3` | that TMR instance | one of four general timers |

Exactly one must be enabled; enabling none, or two, is a `compile_error!`. The
HAL has no default. `examples/ch585` forwards the same feature names and
defaults to `time-driver-systick`; switch with
`--no-default-features --features time-driver-tmr0`, no manifest edit needed.

The SysTick driver has the shortest wake-up path. It reprograms SysTick
(`STRE = 1` plus its own compare value), so the raw counter can no longer be
polled for a delay: measured, `delay_ms(1000)` took **97 ms** alongside an alarm
every 100 ms. `delay::Delay` therefore reads the driver's timebase under this
feature instead of the counter, which is monotonic and correct but costs one
tick of resolution. `delay` works with every driver.

A `time-driver-tmr*` feature instead keeps SysTick free for anything else that
wants a free-running counter, and leaves `delay` on it at full resolution. TMR0
is the least contended instance — TMR1/TMR2 also serve DMA and TMR3 is the one
WCH's BLE stack uses.

A TMR driver's peripheral is removed from `Peripherals` by `build.rs`, so
`init()` can never hand the same instance to a second user.

```toml
ch5xx-hal = { path = "../../", features = ["ch585", "rt", "memory-x", "time-driver-tmr0"] }
embassy-executor = { version = "0.10", features = ["platform-riscv32", "executor-thread", "portable-atomic"] }
embassy-time = { version = "0.5", features = ["tick-hz-1_000_000"] }
portable-atomic = { version = "1.11", features = ["critical-section"] }
```

`ch5xx_hal::init()` then starts the driver, enables its interrupt and turns
global interrupts on, so a `#[qingke_rt::entry]` `main` can spawn tasks on an
`embassy_executor::Executor` straight away.

```console
cd examples/ch585
cargo run --release --bin embassy_time --no-default-features --features time-driver-systick
cargo run --release --bin embassy_time_tmr --no-default-features --features time-driver-tmr0
# toggle 1 at 5 ms / elapsed 501 ms / toggle 2 at 512 ms / ...
```

`embassy_time_tmr` is the one that demonstrates the point of a TMR driver: it
runs an async `Timer` and a blocking `Delay` side by side.

Both counters are "reload at the end value" timers, so they share one state
machine (`src/embassy/reload_timer.rs`): the hardware — not the ISR — decides when
a period ends, so the software high part advances by exactly the programmed
period and interrupt latency never accumulates into the timebase. A far
deadline is split into periods of at most half the counter's range.

### Three things the driver has to work around

All were found by measurement on a CH585; none is obvious from the manuals.

**`wfi` does not wake on a pending interrupt.** QingKe runs `wfi` as `wfe`
whenever `PFIC_SCTLR.WFITOWFE` is set, and `qingke-rt`'s startup sets that bit.
The embassy executor sleeps inside a critical section, where `mstatus.MIE` is
clear, so a timer interrupt there posts no event and the wake waits for
something unrelated. That cost 8–25 ms per alarm — a 500 ms `Timer` fired at
510–627 ms. The driver sets `PFIC_SCTLR.SEVONPEND` to make a pending interrupt
generate the event, which brings the same measurement to ~54 µs and the timer to
501.9 ms. ch32-hal documents the same requirement for its `rt-wfi` feature.

**The clock calibration window was too short.** `delay::init` measures SysTick
against the RTC's 32 kHz counter. With a 1024-count (~31 ms) window the ratio was
so sensitive to the skew between the two reads that it reported anywhere from
5.33 to 7.43 MHz between boots; every delay and the whole embassy timebase were
scaled by that error. The window is now 4096 counts and the median of five
rounds is used.

**The two counters clear their interrupt flag in opposite ways.** SysTick's
`STK_SR.CNTIF` is RW0 — write 0 to clear, and writing 1 is a no-op. A TMR's
`INT_FLAG.IF_CYC_END` is RW1 — write 1 to clear. Each driver supplies its own
`ReloadTimer::clear_flag`, and getting it backwards leaves the flag set, which
turns the handler into an interrupt storm from the first alarm. A TMR's
`CTRL_MOD.ALL_CLEAR` is also a *held level* rather than a self-clearing strobe:
set it, program `CNT_END`, then release it. Pulsing it in a single write leaves
the counter stuck at 0.

With all three handled, `Timer::after(500 ms)` completes in 501.7–502.1 ms on
SysTick, and 10 consecutive 1 s timers take 10018 ms against a host clock. All
four TMR drivers measure 501 ms on the same test.

> **Logging perturbs timing.** `defmt`-over-RTT writes hold a critical section.
> A `info!` inside a timed loop adds tens of milliseconds to the next wake-up, so
> measure intervals with the loop silent and print afterwards.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
