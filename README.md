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
| CH585 | QingKe V3C | Exercised on hardware |

Register access comes from
[`ch5xx-metapac`](https://github.com/ch32-rs/ch5xx-data). Select exactly one
chip feature, currently `ch585`.

## Driver status

| Driver | Status |
| --- | --- |
| `gpio` | Input, output, flex pins, pulls, port-interrupt waits |
| `rcc` | HSI/HSE, dividers, PLL configuration, clock reporting |
| `uart` | UART0–UART3, blocking and async, typed remapping, `embedded-io` traits |
| `i2c` | I2C0 master, blocking and async, up to 400 kHz |
| `spi` | SPI0/SPI1 master, blocking and async, SPI0 local DMA |
| `adc` | Single conversions, temperature and battery channels, polled DMA scans |
| `pwm` | PWM4–PWM11 duty control |
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
full-speed fallback and isochronous transfers, I2C bus recovery, SPI slave mode
and ADC touch-key mode are not implemented; each module documents its own
limits.

## Quick start

```rust
let p = ch5xx_hal::init(ch5xx_hal::rcc::Config::HSI_PLL_24MHZ.into());
```

The reset clock is HSI 16 MHz divided by 3. Select the final clock during
initialization: runtime changes do not rebuild clock-dependent drivers.

## Required configuration

Build for `riscv32imc-unknown-none-elf`, not `riscv32imac`: the QingKe V3C has
no A extension. Shared atomics use `portable-atomic` with the
`critical-section` feature.

With `embassy` enabled, select exactly one of `time-driver-systick` or
`time-driver-tmr0` … `time-driver-tmr3`; none or several is a compile error.

## Running examples

```console
cd examples/ch585
cargo run --release --bin blinky
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
