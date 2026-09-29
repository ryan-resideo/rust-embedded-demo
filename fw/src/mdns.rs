//! mDNS responder, advertising the device on the local network.
//!
//! Answers for `red-<chip id>.local` (the device's address) and for a DNS-SD instance of
//! [`red_proto::udp::SERVICE_TYPE`], so hosts can find every board on the LAN without being
//! told its address. The TXT record carries the chip id and firmware version.

use core::convert::Infallible;
use core::net::Ipv6Addr;

use defmt::{info, warn};
use edge_mdns::HostAnswersMdnsHandler;
use edge_mdns::buf::VecBufAccess;
use edge_mdns::domain::base::Ttl;
use edge_mdns::host::{Host, Service, ServiceAnswers};
use edge_mdns::io::{self, IPV4_DEFAULT_SOCKET};
use edge_nal::UdpSplit;
use edge_nal_embassy::{Udp, UdpBuffers};
use embassy_futures::select::{Either, select};
use embassy_net::Stack;
use embassy_stm32::peripherals::RNG;
use embassy_stm32::rng::Rng;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};

use red_proto::udp;

/// Largest mDNS packet handled: a full Ethernet frame's worth of UDP payload.
const PACKET_LEN: usize = 1472;

/// How long peers may cache the answers.
const TTL: Ttl = Ttl::from_secs(120);

/// Advertises the device over mDNS, for as long as the stack has an address.
///
/// `hostname` is advertised as `<hostname>.local` and as the service instance name.
pub async fn run(stack: Stack<'_>, rng: Rng<'static, RNG>, hostname: &str, chip_id: &str) -> ! {
    let mut rng = MdnsRng(rng);

    // One socket, and its buffers, reused each time the responder restarts
    let buffers = UdpBuffers::<1, PACKET_LEN, PACKET_LEN>::new();
    let recv_buf = VecBufAccess::<NoopRawMutex, PACKET_LEN>::new();
    let send_buf = VecBufAccess::<NoopRawMutex, PACKET_LEN>::new();
    let udp = Udp::new(stack, &buffers);

    let txt = [
        (udp::TXT_CHIP_ID, chip_id),
        (udp::TXT_FW_VERSION, red_core::GIT_VERSION),
    ];
    let service = Service {
        name: hostname,
        priority: 0,
        weight: 0,
        service: udp::SERVICE,
        protocol: udp::PROTOCOL,
        port: udp::PORT,
        service_subtypes: &[],
        txt_kvs: &txt,
    };

    loop {
        // Nothing to advertise without an address
        stack.wait_config_up().await;
        let Some(config) = stack.config_v4() else {
            continue;
        };
        let ip = config.address.address();

        let host = Host {
            hostname,
            ipv4: ip,
            ipv6: Ipv6Addr::UNSPECIFIED,
            ttl: TTL,
        };

        let mut socket = match io::bind(&udp, IPV4_DEFAULT_SOCKET, Some(ip), None).await {
            Ok(socket) => socket,
            Err(e) => {
                warn!("mdns: failed to bind: {}", e);
                Timer::after(Duration::from_secs(1)).await;
                continue;
            }
        };
        let (recv, send) = socket.split();

        // Unused: the answers only change when the address does, which restarts the responder
        let signal = Signal::<NoopRawMutex, ()>::new();
        let mdns = io::Mdns::new(
            Some(ip),
            None,
            recv,
            send,
            &recv_buf,
            &send_buf,
            &mut rng,
            &signal,
        );

        info!("mdns: advertising {}.local at {}", hostname, ip);
        let handler = HostAnswersMdnsHandler::new(ServiceAnswers::new(&host, &service));
        match select(mdns.run(handler), stack.wait_config_down()).await {
            Either::First(Ok(())) => {}
            Either::First(Err(e)) => {
                warn!("mdns: responder failed: {}", e);
                Timer::after(Duration::from_secs(1)).await;
            }
            Either::Second(()) => info!("mdns: address lost"),
        }
    }
}

/// The hardware RNG, behind the `rand_core` version edge-mdns uses to jitter its replies.
struct MdnsRng(Rng<'static, RNG>);

impl rand_core::TryRng for MdnsRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        Ok(self.0.next_u32())
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        Ok(self.0.next_u64())
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        for chunk in dst.chunks_mut(4) {
            chunk.copy_from_slice(&self.0.next_u32().to_le_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}
