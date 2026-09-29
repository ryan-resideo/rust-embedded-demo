# Makefile for building, running, and managing the firmware (fw) and CLI (cli) projects.

fw-build:
	cd fw && cargo build

fw-run:
	cd fw && cargo run

fw-flash:
	cd fw && cargo flash

# On-device tests. Needs a board with a BME280 attached; see fw/tests/.
fw-test:
	cd fw && cargo test

# Builds the on-device tests without a probe, for CI.
fw-test-build:
	cd fw && cargo test --no-run

cli-build:
	cd cli && cargo build

# Pass CLI arguments through ARGS, e.g. `make cli-run ARGS="--transport usb get-device-info"`
cli-run:
	cd cli && cargo run -- $(ARGS)

# Host-side tests against a device attached over USB. Set RED_SERIAL to pick one.
hitl-usb:
	cargo test -p red-lib --test usb -- --ignored

# Host-side tests against a device on the network. Set RED_HOST (chip id or IP) to pick one.
hitl-udp:
	cargo test -p red-lib --test udp -- --ignored

check:
	cargo check
	cd fw && cargo check

fmt:
	cargo fmt
	cd fw && cargo fmt

lint:
	cargo clippy
	cd fw && cargo clippy --all-targets

fix:
	cargo fmt && cargo clippy --fix --allow-dirty
	cd fw && cargo fmt && cargo clippy --fix --allow-dirty

