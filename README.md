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
| CH585 | QingKe V3C | GPIO, clocks, UART and Embassy time drivers exercised on hardware |

Chip support is selected with a Cargo feature (`ch585`). Exactly one must be
enabled.

## Implemented so far

- `init()` — clock/frequency setup and GPIO bring-up
- `gpio` — input, output, flex pins, internal pull up/down, drive strength
- `uart` — blocking and interrupt-driven async UART0–UART3, with pin selection
  checked against the default or remapped pin set at compile time; exact and
  short reads, split TX/RX halves, `embedded-io` traits, and
  `embedded-io-async` traits with the `embassy` feature
- `delay` — `embedded-hal` `DelayNs`, backed by the core SysTick (or by the
  time driver's timebase under `time-driver-systick`)
- `rcc` — HSI/HSE, dividers and PLL configuration; nominal system clock reporting
- Embassy time — SysTick or TMR0–TMR3, with one selected time driver
- Interrupt bindings — `bind_interrupts!` and type-level bindings used by async UART
- Debugging — defmt/RTT, CPU identity and instruction-set probe examples

Experimental Timer counter/PWM/capture and timer-local DMA implementations are
available; see the `timer` and `dma` module docs and the CH585 examples. These
new paths await further real-time qualification, but pass scoped CH585 detached
counter/PWM/capture/DMA/cancellation tests under all five timebase
configurations. They do not yet constitute an NFC transport or a lossless
streaming API.

GPIO interrupt/async waits, SPI, I2C, ADC, USB and BLE are not implemented yet.
RTC is used internally for delay
calibration, not exposed as a user-facing driver. Async UART uses the FIFO and
interrupts, not DMA.

Register access in the generated PAC, or a generated peripheral singleton, does
**not** mean that a HAL driver exists. The CH585 NFC-A vendor archive has been
analyzed offline, but no NFC register model or driver is part of this crate.

## What differs from CH32

The CH5xx register model is not a CH32 one, so a few things look different:

- **No RCC.** Clocks, power and the pin mux live in `SYS` / `System_control` at
  `0x4000_1000`.
- **No AFIO.** Peripheral signals are routed with the `SYS.PIN_ALTERNATE`
  "alternate pin enable" bits rather than a remap register.
- **One GPIO block** with per-port register groups (`pa_out`, `pb_dir`, …)
  instead of one peripheral per port.

## System clock

With the default configuration, the firmware reads `CLK_CFG = 0x0003` — the
datasheet's reset value: `CLK_SYS_MOD = 00`, `CLK_PLL_DIV = 3`, `OSC32M_SEL = 0`,
i.e. **HSI 16 MHz / 3 = 5.333 MHz**. The boot ROM powers the PLL but does not
route the system clock through it.

> **Do not read clock registers with `probe-rs read`.** It halts the core, and a
> 16-bit read of `0x4000_1008` returns `0x014D` (mixing in the neighbouring
> `HFCK_PWR_CTRL` byte) rather than the `0x0003` the firmware sees. Read them
> from firmware and print over RTT — see `examples/ch585`'s `clk_probe`.

`init()` applies `Config.rcc` before starting delay calibration or the Embassy
time driver. Clock presets include `HSI_PLL_24MHZ` and `HSE_PLL_78MHZ`; nearest
achievable frequencies can be selected with `hsi_nearest` / `hse_nearest`.

```rust
let p = ch5xx_hal::init(ch5xx_hal::rcc::Config::HSI_PLL_24MHZ.into());
```

`rcc::clocks().sysclk` reports the **nominal** configured frequency. Separately,
`delay::init` measures SysTick against the RTC's 32 kHz counter at startup (the
same idea as WCH's `OSC_CAL`); `delay::measured_sysclk()` exposes that calibrated
rate for busy-waits. Calibration uses 4096 RTC counts and the median of five
rounds. It still occasionally produces a disturbed median, so it should not be
treated as a precision frequency reference. The Embassy time driver uses the
nominal frequency from `rcc::clocks()`, not this measurement.

`unsafe rcc::set_config` supports runtime clock changes, but updates only the
tree and the nominal frequency cache. It does not rebuild UART dividers or the
Embassy timebase. `delay::recalibrate` is available when the SysTick time driver
is not selected. Prefer selecting the final clock at initialization; HSE presets
require a running 32 MHz crystal (the reference board has one).

The reset clock cannot generate an in-spec 115200 baud. `uart_echo` selects
`HSE_PLL_78MHZ`, giving 114706 baud (-0.43%); see the
[example notes](examples/ch585/README.md#async-uart).

## Target: `riscv32imc`, not `riscv32imac`

The QingKe V3C core is **RV32IMBC — there is no A extension**, and executing
`amoadd.w` / `amoswap.w` raises a hardware exception on real CH585 silicon.
Building for the usual `riscv32imac-unknown-none-elf` therefore produces a
binary that faults the first time any atomic read-modify-write runs. The core
reports this itself: `misa` reads `0x40901106`, i.e. RV32 with `BCIMUX` and no
`A`.

Every atomic in this crate therefore goes through `portable-atomic` with the
`critical-section` feature, which supplies `fetch_add` / `compare_exchange` from
a critical section. Enable the same feature in any binary crate that shares
state with the HAL.

## Examples

See [`examples/ch585`](examples/ch585) for the example binaries, how to run
them, and the console/debugger notes that apply to all of them.

## Embassy time driver

`ch5xx-hal` has an `embassy-time-driver` implementation, selected by a feature:

| Feature | Counter used | Cost |
| --- | --- | --- |
| `time-driver-systick` | core SysTick | takes SysTick from `delay` |
| `time-driver-tmr0` … `time-driver-tmr3` | that TMR instance | one of four general timers |

When `embassy` is enabled, exactly one time driver must be enabled; enabling
none, or two, is a `compile_error!`. The HAL has no default time driver; the
example crate defaults to SysTick. Bare-metal use without `embassy` needs none.

The SysTick driver has the shortest wake-up path. It reprograms SysTick
(`STRE = 1` plus its own compare value), so the raw counter can no longer be
polled for a delay: measured, `delay_ms(1000)` took **97 ms** alongside an alarm
every 100 ms. `delay::Delay` therefore reads the driver's timebase under this
feature instead of the counter, which is monotonic and correct but costs one
tick of resolution. `delay` works with every driver.

A `time-driver-tmr*` feature instead keeps SysTick free for anything else that
wants a free-running counter, and leaves `delay` on it at full resolution.
All four CH585 timers have local DMA. NFC PICC requires TMR0/TMR3, and BLE
examples also use TMR3; select SysTick or an unneeded timer accordingly.

A TMR driver's peripheral is removed from `Peripherals` by `build.rs`, so
`init()` can never hand the same instance to a second user.

The working dependency configuration lives in
[`examples/ch585/Cargo.toml`](examples/ch585/Cargo.toml). This checkout still
uses a generated local `ch5xx-metapac`, local `qingke` / `qingke-rt` patches for
V3C support, and a local Embassy executor for its `portable-atomic` feature.
The executor timer-queue patch in the example is also required to keep a single
compatible queue implementation; this is not yet a registry-only setup.

`ch5xx_hal::init()` then starts the driver, enables its interrupt and turns
global interrupts on, so a `#[qingke_rt::entry]` `main` can spawn tasks on an
`embassy_executor::Executor` straight away.

Both counters are "reload at the end value" timers, so they share one state
machine (`src/embassy/reload_timer.rs`): the hardware — not the ISR — decides when
a period ends, so the software high part advances by exactly the programmed
period. TMR additionally advances the origin by sampled elapsed COUNT at rearm,
because writing CNT_END resets COUNT; SysTick compare writes do not. The small
count-read/end-write bus interval is not a precision-clock guarantee. A far
deadline is split into periods of at most half the counter's range.

### Measured Timebase Pitfalls

All were confirmed by measurement on a CH585; register descriptions alone do
not establish a correct shared timebase implementation.

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
5.33 to 7.43 MHz between boots. Earlier code also used this rate for the Embassy
timebase, scaling it by that error. The window is now 4096 counts and the median
of five rounds is used for busy-waits; the current Embassy timebase instead
uses the nominal configured clock.

**The two counters clear their interrupt flag in opposite ways.** SysTick's
`STK_SR.CNTIF` is RW0 — write 0 to clear, and writing 1 is a no-op. A TMR's
`INT_FLAG.IF_CYC_END` is RW1 — write 1 to clear. Each driver supplies its own
`ReloadTimer::clear_flag`, and getting it backwards leaves the flag set, which
turns the handler into an interrupt storm from the first alarm. A TMR's
`CTRL_MOD.ALL_CLEAR` is also a *held level* rather than a self-clearing strobe:
set it, program `CNT_END`, then release it. Pulsing it in a single write leaves
the counter stuck at 0.

**TMR end writes reset the running count.** A detached RTC regression found that
rearming after 100 ms of blocking work discarded that elapsed time: a requested
500 ms wait took about 600 ms while `Instant` still reported 501 ms. The TMR
backend now declares reset-on-write behavior, and shared rearm arithmetic folds
elapsed COUNT into the software origin and programs the remaining relative
period. The SysTick path retains its compare-only behavior. Independent RTC
histories, not `Instant` differences alone, validate this case.

**Short compares need a post-write check.** Repeated DMA cancellation exposed a
500 us timeout waking only on DMA END about 16 ms later. A fixed lead alone
does not prove COUNT is still before the new compare after flash/bus accesses.
The driver now checks COUNT after programming and retries queue expiry/rearm
when the compare was missed. Five timebase configurations pass bounded detached
stress, but the observed roughly 0.5 ms executor scheduling excess is not an
NFC response-deadline guarantee; NFC still needs its private ISR fast path.

With the original first three handled, `Timer::after(500 ms)` completed in 501.7–502.1 ms on
SysTick, and 10 consecutive 1 s timers take 10018 ms against a host clock. All
four TMR drivers measure 501 ms on the same test.

> **Logging perturbs timing.** `defmt`-over-RTT writes hold a critical section.
> A `info!` inside a timed loop adds tens of milliseconds to the next wake-up, so
> measure intervals with the loop silent and print afterwards.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
