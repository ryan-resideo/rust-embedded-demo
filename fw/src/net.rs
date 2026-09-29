//! The IP network stack, over the board's Ethernet port.
//!
//! The address comes from DHCP. The [`udp`](crate::udp) transport and the
//! [`mdns`](crate::mdns) responder both run on the [`Stack`] built here.

use embassy_net::{Config, DhcpConfig, Runner, Stack, StackResources};
use heapless::String;
use static_cell::StaticCell;

use crate::board::Eth;

/// Sockets the stack can hold: DHCP, the command socket, mDNS, and one spare.
const SOCKETS: usize = 4;

static RESOURCES: StaticCell<StackResources<SOCKETS>> = StaticCell::new();

/// This board's host name: `red-` and its chip id. It is sent to the DHCP server and
/// advertised over mDNS as `<hostname>.local`.
pub fn hostname() -> String<32> {
    let mut name = String::new();
    // 4 + 24 characters, well within the 32 DHCP allows
    name.push_str("red-").unwrap();
    name.push_str(embassy_stm32::uid::uid_hex()).unwrap();
    name
}

/// Builds the network stack on `eth`, configured by DHCP.
///
/// Returns the [`Stack`], a cheap handle for opening sockets, and its [`Runner`], which must
/// be run for the stack to make progress. `seed` should be random, since it picks the stack's
/// initial ports and sequence numbers.
///
/// Panics if called twice: the stack's resources are static.
pub fn init(eth: Eth, seed: u64) -> (Stack<'static>, Runner<'static, Eth>) {
    let mut dhcp = DhcpConfig::default();
    dhcp.hostname = Some(hostname());

    embassy_net::new(
        eth,
        Config::dhcpv4(dhcp),
        RESOURCES.init(StackResources::new()),
        seed,
    )
}
