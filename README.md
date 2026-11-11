# ch5xx-hal

Rust HAL for WCH CH5xx BLE microcontrollers, with blocking APIs and optional
Embassy support.

[API docs](https://docs.rs/ch5xx-hal) · [CH585 examples](examples/ch585) · [CH592 examples](examples/ch592) · [CH582 examples](examples/ch582) · [register metadata](https://github.com/ch32-rs/ch5xx-data)

> **Note**
> Under development; the API may change. CH585, CH592 and CH582 are the chips
> exercised on hardware so far.

## Supported devices

| Chip | Core | Status |
| --- | --- | --- |
| CH585 | QingKe V3C | Board-tested |
| CH592 | QingKe V3C | Board-tested; USBFS enumeration and I2C0 pending |
| CH582 | QingKe V4A | Board-tested; USBFS, RTC, TouchKey and sleep pending |
| CH584 | QingKe V3C | Builds; no board validation |
| CH583 | QingKe V4A | Builds; no board validation |
| CH581 | QingKe V3C | Builds; no board validation |
| CH595 | QingKe V3C | Builds; no board validation |
| CH573 | QingKe V3A | Builds; no board validation |
| CH572 | QingKe V3A | Builds; no board validation |
| CH570 | QingKe V3A | Builds; no board validation |

CH571, CH584X and CH591 are not selectable yet; CH564, CH565 and CH569 are
outside this HAL.

## Driver status

| Driver | Status |
| --- | --- |
| `gpio` | Input, output, flex pins, pulls, port-interrupt waits |
| `rcc` | HSI/HSE, dividers, PLL configuration |
| `uart` | UART0–UART3, blocking and async, remapping, `embedded-io` |
| `i2c` | I2C0 master, blocking and async, up to 400 kHz |
| `spi` | SPI0/SPI1 master, blocking and async, SPI0 local DMA |
| `adc` | Single conversion, temperature, battery, DMA scan, timed auto-DMA |
| `touchkey` | Polled charge-transfer touch keys |
| `pwm` | PWM4–PWM11; PWM1–PWM5 on CH572/CH570 |
| `rtc` | Calendar, one-shot alarm, interval timer |
| `watchdog` | Window watchdog, reset cause, battery detector |
| `delay` | SysTick or the Embassy timebase |
| `power` | Halt and sleep, RTC wake |
| `usb` | USBFS device, EP0–7 (EP0–3 on CH582/CH583) |
| `usbhs` | USBHS device, EP0–7, 8–512-byte bulk packets |
| Embassy time driver | SysTick or TMR0–TMR3 |
| Timer count/PWM/capture/DMA | Experimental |
| USB host, BLE, NFC | Not implemented |

## Quick start

```rust
let p = ch5xx_hal::init(ch5xx_hal::rcc::Config::default().into());
```

Configure the final clock during initialization; runtime changes do not rebuild
clock-dependent drivers.

## Required configuration

- Enable exactly one chip feature, for example `ch585` or `ch592`.
- Use `riscv32imc-unknown-none-elf` for the QingKe V3C parts and
  `riscv32imac-unknown-none-elf` for V3A/V4A. The V3C has no usable A extension.
- Use `portable-atomic` with the `critical-section` feature for shared atomics.
- Do not enable `+b` blindly: CH585 executes it, CH592 traps on it.
- With `embassy`, enable exactly one of `time-driver-systick` or
  `time-driver-tmr0` … `time-driver-tmr3`.
- Enable `memory-x` in binary crates.

## Running examples

```console
cd examples/ch585
cargo run --release --bin blinky
```

See the example READMEs for probe selection and wiring.

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
