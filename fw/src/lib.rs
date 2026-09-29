//! Firmware support library for the NUCLEO-F429ZI.
//!
//! The application lives in `src/main.rs`; everything it is built from lives here so the
//! on-device test suite in `tests/` can bring up the same hardware the same way.

#![no_std]

use defmt_rtt as _; // global defmt logger (RTT transport)
use panic_probe as _; // panic handler (breakpoint; probe-rs reports the halt)

pub mod board;
pub mod channels;
pub mod mdns;
pub mod net;
pub mod platform;
pub mod udp;
pub mod usb;
