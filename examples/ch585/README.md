# CH585 examples

`no_std` example binaries for `ch5xx-hal` on CH585 (QingKe V3C).

Run commands **from this directory**; `.cargo/config.toml` selects the RISC-V
target, `build-std`, and the probe runner.

```console
cd examples/ch585
cargo run --release --bin blinky
```

`cargo run` uses `probe-rs run --chip CH585` to flash, reset, and stream `defmt`
over RTT.

## Time driver selection

The examples default to `time-driver-systick`. Select a TMR driver without
editing the manifest:

```console
cargo run --release --bin embassy_time_tmr --no-default-features --features time-driver-tmr0
```

`embassy_time_tmr` intentionally rejects SysTick, so a default `--bins` build
fails its feature guard. Build all binaries with a TMR driver instead:

```console
cargo build --release --bins --no-default-features --features time-driver-tmr0
```

## Binaries

| Binary | Description |
| --- | --- |
| `blinky` | GPIO output and blocking delay; PB16/PB17 LEDs in opposite phase |
| `gpio_in` | Active-low buttons S1/PB22 and S2/PB23; PB16/PB17 follow S1 |
| `gpio_async` | The same buttons through the port-B interrupt; PB16/PB17 mirror S1/S2 while held |
| `gpio_async_loopback` | Automatic edge check: PA14 output jumpered to PA15 input, 20 cycles, prints PASS/FAIL |
| `defmt_rtt` | Logging over SEGGER RTT |
| `uart_loopback` | Blocking UART0 internal loopback; no external wiring |
| `uart_echo` | Async UART3 echo on PA4/PA5 at 115200 8N1, mirrored to RTT |
| `clk_set` | Boot at `HSI_PLL_24MHZ` and report clock state |
| `embassy_time` | Two tasks using the SysTick time driver |
| `embassy_time_tmr` | Two tasks using a TMR time driver |
| `timer_dma_async` | Timer IRQ binding, DMA waits, and cancellation |
| `i2c_scan` | Blocking I2C0 scan of PB13/PB12; reports every address that ACKs |
| `i2c_oled` | SSD1306 at `0x3C`: bouncing circle and text via embedded-graphics |
| `i2c_oled_async` | Interrupt-driven I2C0 frames on the same SSD1306 |
| `spi_loopback` | Blocking SPI0 loopback; wire PA14 to PA15 |
| `spi_async_loopback` | Async SPI0 full-duplex, then DMA receive and transmit; wire PA14 to PA15 |
| `adc_temp` | Internal temperature sensor, no wiring |
| `adc_pin` | Single conversion on PA4, corrected by the measured offset |
| `adc_scan_dma` | Three-channel scan stored by the ADC-local DMA engine, polled completion |
| `pwm_out` | PWM4 on PA12; breathes an external LED through the duty range at about 400 Hz |
| `rtc_calendar` | Set the real-time clock, read it back, then check the alarm and timer |
| `rtc_oled` | SSD1306 clock: date, `HH:MM:SS`, and a bar that fills each minute |
| `watchdog` | Window watchdog: overflow reset on first boot, then the feed and battery checks |
| `sleep_wake` | Halt and sleep, waking on the RTC; reports the measured interval and checks the clock afterwards |
| `usb_cdc_echo` | CDC ACM serial device on PB11/PB10; echoes each received packet |
| `usbhs_cdc_echo` | High-speed CDC ACM device on PB13/PB12 |
| `usb_endpoints` | Vendor-class full-speed device that exercises endpoints 1 to 7: three echo pairs and a pattern source |
| `usbhs_endpoints` | The same endpoint coverage on the high-speed block |
