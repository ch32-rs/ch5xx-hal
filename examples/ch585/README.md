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
| `gpio_async` | The same buttons through the port-B interrupt |
| `defmt_rtt` | Logging over SEGGER RTT |
| `uart_loopback` | Blocking UART0 internal loopback; no external wiring |
| `uart_echo` | Async UART3 echo on PA4/PA5 at 115200 8N1, mirrored to RTT |
| `clk_set` | Boot at `HSI_PLL_24MHZ` and report clock state |
| `embassy_time` | Two tasks using the SysTick time driver |
| `embassy_time_tmr` | Two tasks using a TMR time driver |
| `timer_dma_async` | Timer IRQ binding, DMA waits, and cancellation |
| `i2c_scan` | Blocking I2C0 scan of PB13/PB12; reports every address that ACKs |
| `i2c_oled` | SSD1306 at `0x3C`: bouncing circle and text via embedded-graphics |

## Notes

### GPIO interrupts

`gpio_async` waits for edges on S1 and S2. PB22/PB23 share flag bits 9:8 with
PB8/PB9, and `gpio_async` selects the button pair. PB16–PB21 have no interrupt
source. One pin watches one edge at a time, so a press and its release are two
waits. Contact bounce logs several edges per press.

### I2C

`i2c_scan` runs I2C0 at 100 kHz on PB13 (SCL) and PB12 (SDA). Both lines need an
external pull-up. A device that acknowledges has its 7-bit address logged over
RTT; a bus with nothing on it logs no address. The remapped pair is PB21/PB20,
selected by constructing the driver with remap `1`.

`i2c_oled` drives the SSD1306 found at `0x3C` at 400 kHz. A white banner carries
the title and a filled circle bounces over the rest of the panel. A 128x32 panel
needs a different multiplex ratio.

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
