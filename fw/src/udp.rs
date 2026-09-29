//! UDP transport: requests and responses as datagrams on [`red_proto::udp::PORT`].
//!
//! Each datagram carries exactly one COBS frame, so unlike USB there is nothing to reassemble.
//! Any number of hosts may talk to the device at once; each is given a slot in a small peer
//! table, and the slot index travels with its requests as the [`Address`] endpoint so the
//! engine's responses find their way back.
//!
//! [`Address`]: crate::channels::Address

use core::cell::Cell;

use defmt::{debug, warn};
use embassy_futures::select::{Either, select};
use embassy_net::udp::{PacketMetadata, RecvError, UdpSocket};
use embassy_net::{IpEndpoint, Stack};
use static_cell::ConstStaticCell;

use red_proto::{MAX_FRAME_LEN, req::Req, udp};

use crate::channels::{Port, Response};

/// Number of hosts the transport can track at once.
///
/// Beyond this, the least recently added host loses its slot. A response still in flight
/// to it then goes to the newcomer, whose client discards it as the id will not match.
const PEERS: usize = 4;

/// Datagrams each socket buffer can queue.
const QUEUE_LEN: usize = 4;

const BUFFER_LEN: usize = QUEUE_LEN * MAX_FRAME_LEN;

static RX_META: ConstStaticCell<[PacketMetadata; QUEUE_LEN]> =
    ConstStaticCell::new([PacketMetadata::EMPTY; QUEUE_LEN]);
static RX_BUFFER: ConstStaticCell<[u8; BUFFER_LEN]> = ConstStaticCell::new([0; BUFFER_LEN]);
static TX_META: ConstStaticCell<[PacketMetadata; QUEUE_LEN]> =
    ConstStaticCell::new([PacketMetadata::EMPTY; QUEUE_LEN]);
static TX_BUFFER: ConstStaticCell<[u8; BUFFER_LEN]> = ConstStaticCell::new([0; BUFFER_LEN]);

/// The command socket.
pub struct UdpTransport {
    socket: UdpSocket<'static>,
}

impl UdpTransport {
    /// Opens the command socket on `stack`, listening on [`udp::PORT`] on every address.
    ///
    /// The stack need not be configured yet: the socket accepts datagrams once DHCP has
    /// assigned an address.
    ///
    /// Panics if called twice: the socket buffers are static.
    pub fn new(stack: Stack<'static>) -> Self {
        let mut socket = UdpSocket::new(
            stack,
            RX_META.take(),
            RX_BUFFER.take(),
            TX_META.take(),
            TX_BUFFER.take(),
        );
        // Only fails if the socket is already bound, or the port is 0
        defmt::unwrap!(socket.bind(udp::PORT));

        Self { socket }
    }

    /// Services requests from any host through `port`.
    pub async fn run(&mut self, port: Port) -> ! {
        let peers = Peers::new();

        let (Either::First(never) | Either::Second(never)) = select(
            receive(&self.socket, &peers, &port),
            transmit(&self.socket, &peers, &port),
        )
        .await;
        never
    }
}

/// Decodes each datagram as a request and submits it to the engine, tagged with its
/// sender's slot.
async fn receive(socket: &UdpSocket<'_>, peers: &Peers, port: &Port) -> ! {
    let mut frame = [0u8; MAX_FRAME_LEN];

    loop {
        let (n, meta) = match socket.recv_from(&mut frame).await {
            Ok(r) => r,
            Err(RecvError::Truncated) => {
                warn!("udp: dropped oversized datagram");
                continue;
            }
        };

        let req = match Req::decode(&mut frame[..n]) {
            Ok(req) => req,
            Err(_) => {
                warn!("udp: dropped undecodable datagram from {}", meta.endpoint);
                continue;
            }
        };

        debug!("udp: request {} from {}", req, meta.endpoint);
        port.request(peers.slot(meta.endpoint), req).await;
    }
}

/// Encodes the engine's responses and sends each back to the host its request came from.
async fn transmit(socket: &UdpSocket<'_>, peers: &Peers, port: &Port) -> ! {
    let mut frame = [0u8; MAX_FRAME_LEN];

    loop {
        let Response { address, resp } = port.receive().await;

        let Some(peer) = peers.get(address.endpoint) else {
            warn!(
                "udp: no peer in slot {} for response {}",
                address.endpoint, resp.id
            );
            continue;
        };

        let n = match resp.encode(&mut frame) {
            Ok(n) => n,
            Err(_) => {
                warn!("udp: failed to encode response {}", resp.id);
                continue;
            }
        };

        if let Err(e) = socket.send_to(&frame[..n], peer).await {
            warn!(
                "udp: failed to send response {} to {}: {}",
                resp.id, peer, e
            );
        }
    }
}

/// The hosts talking to the device, each identified by its slot index.
///
/// Both halves of the transport run in one task, so a [`Cell`] is all the sharing needs.
struct Peers {
    slots: Cell<[Option<IpEndpoint>; PEERS]>,
    /// The slot the next new host takes, cycling round so the oldest is replaced first.
    next: Cell<usize>,
}

impl Peers {
    const fn new() -> Self {
        Self {
            slots: Cell::new([None; PEERS]),
            next: Cell::new(0),
        }
    }

    /// The slot for `peer`, allocating one if it is new.
    fn slot(&self, peer: IpEndpoint) -> u16 {
        let mut slots = self.slots.get();

        let index = match slots.iter().position(|s| *s == Some(peer)) {
            Some(index) => index,
            None => {
                let index = self.next.get();
                self.next.set((index + 1) % PEERS);
                slots[index] = Some(peer);
                self.slots.set(slots);
                index
            }
        };

        index as u16
    }

    /// The host in `slot`, if there is one.
    fn get(&self, slot: u16) -> Option<IpEndpoint> {
        self.slots.get().get(slot as usize).copied().flatten()
    }
}
