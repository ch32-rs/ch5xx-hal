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
| `gpio_in` | Active-low buttons S1/PB22 and S2/PB23 with pull-ups |
| `defmt_rtt` | Logging over SEGGER RTT |
| `cpuid` | Core identity and ISA CSRs |
| `bitmanip` | Hardware probe for ratified bit-manipulation instructions |
| `uart_loopback` | Blocking UART0 internal loopback; no external wiring |
| `uart_echo` | Async UART3 echo on PA4/PA5 at 115200 8N1, mirrored to RTT |
| `clk_probe` | Clock registers, nominal clock, and measured SysTick rate |
| `clk_set` | Boot at `HSI_PLL_24MHZ` and report clock state |
| `embassy_time` | Two tasks using the SysTick time driver |
| `embassy_time_tmr` | Two tasks using a TMR time driver |
| `timer_dma` | Timer counter/PWM/capture/DMA characterization |
| `timer_dma_async` | Timer IRQ binding, DMA waits, and cancellation |
| `timer_acceptance` | Detached SRAM-recorded Timer/DMA acceptance checks |
| `timer_clock_guard` | Reject the NFC clock when the system clock is too slow |
| `timer_timebase` | Async/blocking waits checked against the independent RTC |
| `timer_dma_stress` | Repeated short transfers, cancellation, and failure paths |
| `timer_external_capture` | Capture qualification from an independent 3.3 V UART source |

## Notes

### UART

`uart_echo` uses HSE PLL 78 MHz because the reset 5.333 MHz clock cannot generate
an in-spec 115200 baud. Connect the external device at 3.3 V logic: TX to PA4
(P2.1), RX to PA5 (P2.2), and share GND. The async driver also provides
`read_some` for short, cancel-safe reads; see the `uart` module documentation.

### Timer

Disconnect external loads from PA9/PA10/PA11/PA2 before output tests. The
acceptance binaries record results in SRAM rather than streaming timing data
over RTT. `timer_external_capture` ties these pads together as inputs; follow
the wiring and safety notes at the top of its source and remove all fan-out
wires before running output examples.

### Debugging

`DEFMT_LOG = "info"` is set in `.cargo/config.toml`. Debugger memory reads halt
the core and do not resume it, so do not poll live counters through `probe-rs`
or `wlink`; record a history in RAM and read it once.
