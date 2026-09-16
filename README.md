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

