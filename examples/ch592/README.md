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
| `adc_temp` | Internal temperature sensor, single conversion |
| `clk_pll48` | 48 MHz from the 480 MHz PLL, Flash timing and read-back |
| `embassy_time` | Embassy time driver; pick SysTick or TMR0–TMR3 with features |
| `i2c_scan` | I2C0 address scan on PB15/PB14 |
| `rtc_calendar` | RTC calendar, one-shot alarm and interval timer |
| `sleep_wake` | `power::halt`/`power::sleep` with RTC wake, self-checked |
| `spi_loopback` | SPI0 mode 0 and mode 3 loopback; jumper PA14 to PA15 on P2 |
| `timer_counter` | `timer` counters on a no-DMA (TMR0) and a full (TMR2) block |
| `uart_loopback` | UART0 internal loopback, self-checked |
| `usb_cdc_echo` | USB full-speed CDC ACM echo on PB11/PB10; needs a host cable |
| `watchdog` | Window watchdog overflow reset, feed path and battery detector |

Self-checking examples log a `SELFTEST PASS` or `SELFTEST FAIL` line, following
the CH585 bench tests: drive both ends inside the chip or with one jumper, then
let the firmware compare what came back.

Build for a TMR timebase without editing the manifest:

```console
cargo build --release --bin embassy_time --no-default-features --features time-driver-tmr1
```

All four TMR instances work as Embassy timebases, including TMR0/TMR3, whose
register block omits the timer-local DMA registers. The `timer` and `dma`
modules now compile on this die; counter, capture, PWM and interrupt APIs are
available, while `DmaInstance` keeps the DMA entry points off the no-DMA
instances at compile time.

## Notes

- The probe cannot reach the core while it is stopped, so `sleep_wake` ends its
  RTT session at the first sleep and records its verdict in SRAM instead. Build
  it without the Embassy time driver so SysTick belongs to the blocking delay:
  `cargo run --release --bin sleep_wake --no-default-features`, then read the
  address printed before the first sleep with `probe-rs read`.
- `usb_cdc_echo` needs the board's USB-C connector wired to a host; the debug
  probe alone does not supply VBUS. Until a host attaches it logs
  `usb attach false` and stays idle.
- `spi_loopback` needs a jumper from P2 pin 14 (PA14, MOSI) to pin 16 (PA15,
  MISO). PB14/PB15 are the I2C pair on this die and are also the 2-wire debug
  interface while the ISP-config debug enable stays set, so `i2c_scan` sees
  spurious arbitration-loss flags while probe-rs is attached; a real I2C run
  needs the debug interface disabled or a wired slave.
- `probe-rs` now attaches to this die; older versions failed with a bulk-read
  timeout while the stock firmware held the debug pins.
- The HAL also implements the WCH-Link SDI print channel on CH592
  (`hal::debug::SDIPrint`), but the examples use defmt/RTT so logs are captured
  by the same `cargo run` that flashes.
- A `+b` target feature traps on this die; the crate builds with no extra
  target features.
