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
| `usb_cdc_echo` | CDC ACM serial device on PB11/PB10; echoes each received packet |
| `usbhs_cdc_echo` | High-speed CDC ACM device on PB13/PB12 |

## Notes

### GPIO interrupts

`gpio_async` waits for edges on S1 and S2. PB22/PB23 share flag bits 9:8 with
PB8/PB9, and `gpio_async` selects the button pair. PB16–PB21 have no interrupt
source. One pin watches one edge at a time, so a press and its release are two
waits. Contact bounce logs several edges per press.

`gpio_async_loopback` exercises the same driver without an operator: jumper PA14
to PA15 and it drives 20 cycles, counts the edges it sees, and prints
`SELFTEST PASS` or the two counts. Both pins are on port A, so the PB22/PB23
`PIN_INTX` alias is covered only by `gpio_async`.

### I2C

`i2c_scan` runs I2C0 at 100 kHz on PB13 (SCL) and PB12 (SDA). Both lines need an
external pull-up. A device that acknowledges has its 7-bit address logged over
RTT; a bus with nothing on it logs no address. The remapped pair is PB21/PB20,
selected by constructing the driver with remap `1`.

`i2c_oled` drives the SSD1306 found at `0x3C` at 400 kHz. A white banner carries
the title and a filled circle bounces over the rest of the panel. A 128x32 panel
needs a different multiplex ratio.

`i2c_oled_async` uses the same wiring and init sequence, but every frame is an
interrupt-driven async write. It alternates vertical bars and a diagonal pattern
once per second. I2C0 has no DMA engine, so this is the async path; the `I2C0`
interrupt is bound with `bind_interrupts!`.

### SPI

`spi_loopback` runs SPI0 at 1 MHz on PA13 (SCK), PA14 (MOSI) and PA15 (MISO).
Join PA14 to PA15. It exchanges 10 bytes in mode 0, then 40 bytes in mode 3, and
logs whether each one comes back unchanged. PA12 is driven as the chip select
even though the loop needs none. The remapped set is PB13/PB14/PB15, selected by
constructing the driver with remap `1`.

`spi_async_loopback` repeats the full-duplex check through the interrupt path,
then uses SPI0's local DMA for a 5000-byte receive and a 5000-byte transmit. The
receive must read `0xFF`: with `FIFO_DIR=1` the controller clocks `0xFF` out on
MOSI, and the wire loops it back. The transmit checks the DMA engine's `.wait()`
completion and its internal re-arm past the 12-bit `TOTAL_CNT` limit; verifying
the wire pattern needs an analyzer. The DMA path is half-duplex by design, so a
full-duplex transfer always uses the interrupt path.

### ADC

`adc_temp` reads the internal temperature sensor and logs the raw count and the
temperature in Celsius. It needs no wiring. The conversion uses the calibration
value stored in the chip's info flash.

`adc_pin` reads PA4, channel 0. Tie the pin to ground or to 3.3 V; left open, the
count floats. The log adds the offset measured once at start with the converter's
input shorted.

`adc_scan_dma` scans PA4, PA5 and PA6, which are channels 0, 1 and 10. The tuple
order is the scan order. Tie PA4 to ground and PA5 to 3.3 V for two known
readings; PA6 may float. Each buffer holds ten passes of three samples and is
static, because the ADC-local DMA engine writes it while the CPU is not
borrowing it. One scan is logged every 500 ms. Completion is polled, because the
ADC interrupt line is also asserted by end-of-conversion and the `IE_*` bits do
not mask it, so an interrupt-driven wait spends its time in a handler that
cannot clear the line without disturbing the sequence. The line is shared with
the touch-key block, so a later touch-key driver must account for this.

### PWM

`pwm_out` drives PWM4 on PA12. The on-board PB16/PB17 LEDs are GPIO-only and
have no PWM alternate function, so use an external LED: PA12 through a
resistor (330 ohms to 1 kohm) to the anode, cathode to GND. At about 400 Hz the
eye sees average brightness, and the duty ramps 0 to 255 and back. A probe
shows the same step as a changing pulse width. The shared remap moves PWM4 to
PA6.

### RTC

`rtc_calendar` needs no wiring. It loads 2026-11-06 12:00:00, logs the calendar
four times one second apart, then sets an alarm one second ahead and starts the
one-second interval timer. The seconds should step 0, 1, 2, 3 and the last line
should report both the alarm and the timer pending. The counter runs from the
internal 32 kHz oscillator and is not cleared by a reset.

`rtc_oled` uses the same panel as `i2c_oled`: SSD1306 at `0x3C` on PB13/PB12,
with external pull-ups. It loads 12:00:00 and redraws when the RTC second
changes. The date is on the top line, the time under it, and a bar along the
bottom fills across the minute.

### Watchdog

`watchdog` needs no wiring and deliberately resets the board once. The first
boot logs the reset cause, arms overflow reset, and stops feeding; after the
counter wraps (about 6.3 s at the reset clock) the chip reboots and the next
boot logs `reset cause watchdog`. The example then disables reset and checks
that a feed clears the overflow flag, that it sets after four seconds, and that
a second feed clears it again. A USB-powered board should not report a low
battery.

### USB

`usb_cdc_echo` uses P1, the board's TYPE-C FS-USB connector: PB11 is D+ and
PB10 is D-. P4 is the separate HS-USB connector and is not supported by this
example. The WCH-Link remains attached only for flashing and RTT. If your board
cannot draw target power from the WCH-Link, power it through FS-USB while
leaving the Link connected for debugging. The 78 MHz HSI PLL supplies the
device clock, so no crystal is needed. Ordinary packets echo unchanged. The
exact packet `PING` returns `PONG`; `PATTERN <n>` returns `n`
bytes counting modulo 256. Enumeration, binary transfers from 1 through 65536
bytes, packet-size boundaries, and closing and reopening the serial port have
been board-tested.

`usbhs_cdc_echo` instead uses P4, the board's TYPE-C HS-USB connector: PB13 is
D+ and PB12 is D-. It selects the 62.4 MHz HSE PLL, which requires the 32 MHz
crystal, and uses 512-byte bulk packets. It supports the same echo, `PING`, and
`PATTERN <n>` commands. Enumeration at 480 Mb/s, binary transfers from 1 through
65536 bytes, and closing and reopening the serial port have been board-tested.

### UART

`uart_echo` uses HSE PLL 78 MHz because the reset 5.333 MHz clock cannot generate
an in-spec 115200 baud. Connect the external device at 3.3 V logic: TX to PA4
(P2.1), RX to PA5 (P2.2), and share GND. The async driver also provides
`read_some` for short, cancel-safe reads; see the `uart` module documentation.

### Timer

`timer_dma_async` demonstrates the experimental Timer/DMA API. Disconnect
external loads from PA9/PA10/PA11/PA2 before running it. The driver is not yet
qualified for hard real-time protocols or lossless streaming.

### Debugging

`DEFMT_LOG = "info"` is set in `.cargo/config.toml`. Debugger memory reads halt
the core and do not resume it, so do not poll live counters through `probe-rs`
or `wlink`; record a history in RAM and read it once.
