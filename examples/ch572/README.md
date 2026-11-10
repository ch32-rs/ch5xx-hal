# CH572 examples

`no_std` example binaries for `ch5xx-hal` on CH572/CH570 (QingKe V3A).

Run commands **from this directory**; `.cargo/config.toml` selects the RISC-V
target, `build-std`, and the probe runner.

```console
cd examples/ch572
cargo build --release --bin pwm_selftest
cargo run --release --bin pwm_selftest
```

`cargo run` uses `probe-rs run --chip CH572` to flash, reset, and stream
`defmt` over RTT. With several WCH-Links attached, select yours with
`PROBE_RS_PROBE=<VID:PID:SERIAL>`.

## Binaries

| Binary | Description |
| --- | --- |
| `pwm_selftest` | PWM1-PWM5 register readback for fixed and 16-bit cycles; no wiring |
