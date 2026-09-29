# Rust Embedded Demo

A demo project to demonstrate Rust for embedded-applications, designed to support the presentation [here](https://resideoinc-my.sharepoint.com/:p:/r/personal/ryan_kurte_resideo_com/Documents/embedded-rust.pptx?d=w6f06c33cc9df47df819fe25a03f20fbe&csf=1&web=1&e=6ybT3U).
If you don't have access to this, send an email to `ryan.kurte@resideo.com`.

This is intended to follow the progression of the talk. To see this step-by-step, checkout the [PRs](https://github.com/ryan-resideo/rust-embedded-demo/pulls).

## Status

[![CI](https://github.com/ryan-resideo/rust-embedded-demo/actions/workflows/ci.yml/badge.svg)](https://github.com/ryan-resideo/rust-embedded-demo/actions/workflows/ci.yml)

## Layout

- [fw](fw/) contains the STM32 devices firmware, which serves requests over USB and Ethernet (UDP)
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
(1/4) running `bme280_chip_id`...
(2/4) running `stm32_uid`...
(3/4) running `eth_phy_id`...
(4/4) running `platform_measures`...
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

## Talking to the device

The firmware exposes a vendor-specific USB interface on the NUCLEO's **user USB** connector
(CN13, at the bottom of the board), not the ST-Link one. With both plugged in:

```bash
cargo run -p red-cli -- --transport usb list-devices
cargo run -p red-cli -- --transport usb get-device-info
cargo run -p red-cli -- --transport usb get-measurement
```

The USB serial number is the chip's unique id, so with several boards attached pick one with
`--host <serial>`. A measurement is taken every 10 seconds; until the first one,
`get-measurement` reports `MeasurementFailed`.

Requests and responses are the `red-proto` types, postcard-encoded and COBS-framed, so a
message may span several 64 byte USB packets. On the host, `red-lib` uses
[`nusb`](https://crates.io/crates/nusb), which needs no driver on any platform:

- macOS: nothing to do.
- Windows: the device's MS OS 2.0 descriptors bind WinUSB automatically.
- Linux: grant access to non-root users with a udev rule, e.g. in
  `/etc/udev/rules.d/70-red.rules`:
  `SUBSYSTEM=="usb", ATTR{idVendor}=="c0de", ATTR{idProduct}=="cafe", MODE="0666"`

`make hitl-usb` runs the host-side tests in `lib/tests/usb.rs` against an attached board.

The VID/PID (`0xc0de`/`0xcafe`, in `proto/src/usb.rs`) are the embassy examples' demo pair,
not an allocated ID.

### Over Ethernet

Plug the NUCLEO's RJ45 connector (CN14) into a network with a DHCP server. The board takes an
address, serves requests on UDP port 7477, and advertises itself over mDNS as
`red-<chip id>.local`, with a `_red._udp` DNS-SD service. The host finds it from there:

```bash
cargo run -p red-cli -- --transport udp list-devices
cargo run -p red-cli -- --transport udp get-device-info
cargo run -p red-cli -- --transport udp get-measurement
```

With several boards on the network, pick one with `--host <chip id>`. Discovery needs
multicast between the host and the board, which guest Wi-Fi and many managed networks block;
`--host` also takes an address (`--host 192.168.1.20`, or `IP:port`), which skips mDNS
entirely. The board logs its address over RTT once DHCP completes, and
`dns-sd -B _red._udp` (macOS) or `avahi-browse -r _red._udp` (Linux) shows what is being
advertised.

Each datagram carries exactly one COBS-framed `red-proto` message, the same encoding as USB.
UDP can lose datagrams, so a lost request or response shows up as a timeout; just retry.

The MAC address is locally administered, derived from the chip's unique id, so each board
keeps the same address (and DHCP lease) across reboots.

`make hitl-udp` runs the host-side tests in `lib/tests/udp.rs` against a board on the
network. Set `RED_HOST` to a chip id or an address to pick one.

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

