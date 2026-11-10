# ch5xx-hal

Rust HAL for WCH CH5xx BLE microcontrollers, with blocking APIs and optional
Embassy support.

[Documentation](https://docs.rs/ch5xx-hal) · [CH585 examples](examples/ch585) · [CH592 examples](examples/ch592) · [CH582 examples](examples/ch582)

> **Note**
> This crate is under development and its API may change. CH585, CH592 and
> CH582 are the chips exercised on hardware so far.

## Supported devices

| Chip | Core | Status |
| --- | --- | --- |
| CH585 | QingKe V3C | Exercised on hardware |
| CH592 | QingKe V3C | Clocks, delay, GPIO, UART0, SPI0, ADC, RTC, watchdog, PWM, timers, power and TouchKey exercised on hardware; USBFS builds, host enumeration and I2C0 pending |
| CH582 | QingKe V4A | GPIO, UART0, SPI0, I2C0, ADC, watchdog, timers and PWM exercised on hardware; USBFS, RTC, TouchKey and sleep pending |
| CH584 | QingKe V3C | Builds; no board validation |
| CH583 | QingKe V4A | Builds; no board validation |
| CH581 | QingKe V3C | Builds; no board validation |
| CH595 | QingKe V3C | Builds; no board validation |
| CH573 | QingKe V3A | Builds; no board validation |
| CH572 | QingKe V3A | Builds; no board validation |
| CH570 | QingKe V3A | Builds; no board validation |

CH571, CH584X and CH591 reuse these family files but remove or add blocks the
shared metadata does not model, so they do not have a feature yet. CH564, CH565
and CH569 use different cores or GPIO layouts and are outside this HAL.

Register access comes from
[`ch5xx-metapac`](https://github.com/ch32-rs/ch5xx-data). Select exactly one
chip feature, for example `ch585` or `ch592`.

## Driver status

| Driver | Status |
| --- | --- |
| `gpio` | Input, output, flex pins, pulls, port-interrupt waits |
| `rcc` | HSI/HSE, dividers, PLL configuration, clock reporting |
| `uart` | UART0–UART3, blocking and async, typed remapping, `embedded-io` traits |
| `i2c` | I2C0 master, blocking and async, up to 400 kHz |
| `spi` | SPI0/SPI1 master, blocking and async, SPI0 local DMA |
| `adc` | Single conversions, temperature and battery channels, polled DMA scans on CH585 and CH592, timed auto-DMA |
| `touchkey` | Charge-transfer touch keys through the ADC, polled |
| `pwm` | PWM4–PWM11 duty, 16-bit cycle and duty on PWM4–PWM9, channel-pair stagger |
| `rtc` | Calendar, one-shot alarm, interval timer |
| `watchdog` | Window watchdog, reset cause, battery detector |
| `delay` | SysTick or the Embassy timebase |
| `power` | Halt and sleep, RTC wake |
| `usb` | USBFS device, endpoints 0–7, packets up to 64 bytes |
| `usbhs` | USBHS device, endpoints 0–7, high-speed bulk 8–512 bytes |
| Embassy time driver | SysTick or TMR0–TMR3, typed interrupt bindings |
| Timer count/PWM/capture/DMA | Experimental |
| USB host, BLE, NFC | Not implemented |

DMA engines are peripheral-local. USBFS suspend/resume, USBHS dynamic
full-speed fallback and isochronous transfers, I2C bus recovery and SPI slave
mode are not implemented; each module documents its own limits. Blocks that
differ per die select a variant (CH592 uses `pwmx_ch592`, `usb_ch592`,
`power_sleep_ch59x` and the mixed FULL/NODMA timer blocks) or are absent there
(SPI1 and USBHS). LCD, BLE and NFC have no driver on any die.

## Quick start

```rust
let p = ch5xx_hal::init(ch5xx_hal::rcc::Config::default().into());
```

The reset clock is HSI 16 MHz divided by 3 on CH581/CH584/CH585/CH595, and the
32 MHz crystal divided by 5 (6.4 MHz) on CH572/CH573/CH592/CH58x, which have no
internal high-speed oscillator.
Select the final clock during initialization (`Config::HSI_PLL_*`,
`Config::PLL_*`, `Config::hse_nearest`): runtime changes do not rebuild
clock-dependent drivers.

## Required configuration

Select the target by core: `riscv32imc-unknown-none-elf` for the QingKe V3C
(CH581, CH584, CH585, CH592, CH595) and `riscv32imac-unknown-none-elf` for the
V3A/V4A parts (CH570, CH572, CH573, CH582, CH583). The V3C has no usable A
extension.
Shared atomics use `portable-atomic` with the `critical-section` feature.

Do not enable RISC-V extensions blindly: CH585 reports the B extension and
executes `+b` code, while CH592 reports A and no B, and a `+b` build traps as an
illegal instruction. The other V3C parts have not been checked for B support.

With `embassy` enabled, select exactly one of `time-driver-systick` or
`time-driver-tmr0` … `time-driver-tmr3`; none or several is a compile error.

## Running examples

```console
cd examples/ch585
cargo run --release --bin blinky

cd examples/ch592
cargo run --release --bin hello
```

The CH59x board is flashed and logged through `probe-rs` over its WCH-Link
(`probe-rs run --chip CH592`); see
[`examples/ch592/README.md`](examples/ch592/README.md) for probe selection.

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
