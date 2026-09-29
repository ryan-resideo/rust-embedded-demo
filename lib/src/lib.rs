//! Talking to the RED device from a host.
//!
//! A [`Link`] carries requests and responses over some transport; a [`Client`] runs the
//! request/response exchange over any link.

use clap::ValueEnum;

mod client;
mod error;
mod link;
pub mod usb;

pub use client::Client;
pub use error::Error;
pub use link::{Link, StreamLink};

/// Transports supported by the RED comms library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Transport {
    /// Connect via USB
    Usb,
    /// Connect via IP/UDP
    Udp,
}
