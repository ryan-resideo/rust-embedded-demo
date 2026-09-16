# Makefile for building, running, and managing the firmware (fw) and CLI (cli) projects.

fw-build:
	cd fw && cargo build

fw-run:
	cd fw && cargo run

fw-flash:
	cd fw && cargo flash

cli-build:
	cd cli && cargo build

cli-run:
	cd cli && cargo run

check:
	cargo check
	cd fw && cargo check

fmt:
	cargo fmt
	cd fw && cargo fmt

lint:
	cargo clippy
	cd fw && cargo clippy

fix:
	cargo fmt && cargo clippy --fix --allow-dirty
	cd fw && cargo fmt && cargo clippy --fix --allow-dirty
