# ch5xx-hal

Rust HAL for WCH CH5xx BLE microcontrollers, with blocking APIs and optional
Embassy support.

[Documentation](https://docs.rs/ch5xx-hal) · [Examples](examples/ch585)

> **Note**
> This crate is under development and its API may change. CH585 is the only
> chip exercised on hardware so far.

## Supported devices

| Chip | Core | Status |
| --- | --- | --- |
| CH585 | QingKe V3C | GPIO, clocks, UART, and Embassy time drivers tested on hardware |

Select exactly one chip feature, currently `ch585`. Register access comes from
[`ch5xx-metapac`](https://github.com/ch32-rs/ch5xx-data), allowing shared driver
code across chips with compatible peripherals.

## Features

- `gpio`: input, output, flex pins, pulls, drive strength, and port-interrupt
  waits for level and single-edge triggers.
- `rcc`: HSI/HSE, dividers, PLL configuration, and clock reporting.
- `uart`: blocking and interrupt-driven async UART0–UART3, typed pin remapping,
  split halves, exact and short reads, and `embedded-io`/`embedded-io-async`
  traits.
- `delay`: `embedded-hal` `DelayNs` using SysTick or the Embassy timebase.
- Embassy time driver on SysTick or TMR0–TMR3, plus typed interrupt bindings.

Timer counter/PWM/capture and timer-local DMA drivers are experimental. They
pass scoped detached CH585 tests but are not yet qualified for arbitrary bus
loads, lossless streaming, or NFC transport.

I2C, SPI, ADC, USB, BLE, and a public RTC calendar/alarm API are not
implemented yet.

## Architecture

The API and ownership model follow `ch32-hal` and Embassy, with CH5xx-specific
hardware differences:

- Clocks and pin muxing live in `SYS`, not separate RCC/AFIO peripherals.
- One GPIO block contains per-port register groups.
- Each timer has one 26-bit counter/FIFO, no prescaler, and peripheral-local DMA.

A generated PAC singleton does not imply that a HAL driver exists.

## Quick start

```rust
let p = ch5xx_hal::init(ch5xx_hal::rcc::Config::HSI_PLL_24MHZ.into());
```

The default clock is HSI 16 MHz divided by 3 (5.333 MHz). It cannot generate an
in-spec 115200 baud for external UART communication; `uart_echo` therefore uses
`HSE_PLL_78MHZ`. HSE presets require a 32 MHz crystal.

Prefer selecting the final clock during initialization. Runtime clock changes
do not rebuild UART dividers, the Embassy timebase, or other clock-dependent
drivers.

See [`examples/ch585`](examples/ch585) for complete programs and wiring notes.

## Embassy time driver

| Feature | Counter | Effect |
| --- | --- | --- |
| `time-driver-systick` | Core SysTick | `delay` uses the Embassy timebase |
| `time-driver-tmr0` … `time-driver-tmr3` | Selected TMR | SysTick remains available for `delay` |

With `embassy` enabled, exactly one time driver must be selected; none or
multiple selections produce a compile error. The HAL has no default time driver,
while the example crate defaults to SysTick. A selected TMR is removed from
`Peripherals`, preventing a second owner.

`init()` starts the selected driver and enables interrupts, so an Embassy
executor can be started directly from `main`.

## Target and atomics

The QingKe V3C is RV32IMBC with **no A extension**. Build for
`riscv32imc-unknown-none-elf`, not `riscv32imac`, and use `portable-atomic` with
the `critical-section` feature for shared atomics. The example crate already has
the required target and dependency configuration.

## Running examples

```console
cd examples/ch585
cargo run --release --bin blinky
```

The runner uses `probe-rs run --chip CH585` to flash, reset, and stream `defmt`
over RTT. The example manifest also carries the local metapac, qingke, and
Embassy dependency patches required by this checkout. To build every example,
select a TMR time driver:

```console
cargo build --release --bins --no-default-features --features time-driver-tmr0
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
