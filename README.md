# Rust Embedded Demo

A demo project to demonstrate Rust for embedded-applications, designed to support the presentation [here](https://resideoinc-my.sharepoint.com/:p:/r/personal/ryan_kurte_resideo_com/Documents/embedded-rust.pptx?d=w6f06c33cc9df47df819fe25a03f20fbe&csf=1&web=1&e=6ybT3U).
If you don't have access to this, send an email to `ryan.kurte@resideo.com`.

This is intended to follow the progression of the talk. To see this step-by-step, checkout the [PRs](https://github.com/ryan-resideo/rust-embedded-demo/pulls).

## Status

[![CI](https://github.com/ryan-resideo/rust-embedded-demo/actions/workflows/ci.yml/badge.svg)](https://github.com/ryan-resideo/rust-embedded-demo/actions/workflows/ci.yml)

## Layout

- [fw](fw/) contains the STM32 devices firmware
- [core](core/) contains application/business logic
- [proto](proto/) defines the protocol for communication between components
- [lib](lib/) supports interacting with the device from different OS'
- [cli](cli/) wraps `lib` to provide a command line interface for talkin' to the device.

## Building

### Dependencies

- `cargo` and `rustc` via [https://rustup.rs/]
- The `thumbv7em-none-eabihf` target, via `rustup target add thumbv7em-none-eabihf`
- `probe-rs` via `cargo install probe-rs-tools` (or `binstall` if you have it)

## Testing

`core`, `proto` and `lib` are host-testable with `cargo test --workspace`. The firmware is
not: it tests on the device itself, with [`defmt-test`](https://crates.io/crates/defmt-test).

```bash
make fw-test
```

That builds `fw/tests/hardware.rs`, flashes it over the probe and runs it in place. Each
`#[test]` reports over RTT and the suite exits through semihosting, so `probe-rs` turns the
result back into an exit code and `cargo test` passes or fails as usual:

```
(1/3) running `bme280_chip_id`...
(2/3) running `stm32_uid`...
(3/3) running `platform_measures`...
all tests passed!
```

These are real hardware tests -- a NUCLEO-F429ZI with a BME280 on I2C1 has to be attached,
and they fail without one. CI has no probe, so it only builds them (`make fw-test-build`).

Two structural notes, both visible in `fw/Cargo.toml`:

- `fw` has a library target (`src/lib.rs`) because Cargo's `tests/` directory can only link
  a library. `src/main.rs` is a thin shell over it, and `board.rs` holds the bring-up both
  the application and the tests share, so they run against the same clock tree and pins.
- Interrupts are bound with `bind_interrupts!` in each binary rather than in the library.
  The macro defines `#[no_mangle]` handlers that the vector table resolves against weak
  defaults, and a definition the linker has no reason to pull out of an rlib loses to that
  default -- silently, with async peripherals that then never complete.

## Logging

The firmware logs over RTT with [`defmt`](https://defmt.ferrous-systems.com/). `probe-rs`
decodes it from the `.defmt` section of the ELF, so nothing beyond `make fw-run` is needed:

- `make fw-run` (`cargo run`) flashes and stays attached, streaming logs.
- `make fw-flash` (`cargo flash`) flashes and detaches — it shows **no** logs. Use
  `probe-rs attach` to reconnect to a board that is already running.

Verbosity is a *compile-time* filter, so changing it rebuilds. `fw/.cargo/config.toml`
defaults it to `debug`; override it per invocation:

```bash
DEFMT_LOG=trace cargo run --release
```

`DEFMT_LOG=off` drops every log call and its interned string from the binary, and module
filters such as `DEFMT_LOG=red_fw=trace,info` work too. Frames are prefixed with uptime in
milliseconds, provided by `embassy-time`'s `defmt-timestamp-uptime-ms` feature.

