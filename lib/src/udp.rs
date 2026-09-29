//! Finding devices on the network with mDNS, and talking to them over UDP.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use tokio::net::UdpSocket;
use tracing::{debug, warn};

use red_proto::{MAX_FRAME_LEN, req::Req, resp::Resp, udp};

use crate::{Error, Link};

/// How long [`discover`] listens for devices by default.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);

/// A device found on the network by [`discover`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// The device's chip id, from its TXT record
    pub chip_id: String,
    /// The DNS-SD instance name, `red-<chip id>`
    pub instance: String,
    /// Where the device serves requests
    pub addr: SocketAddr,
    /// The firmware version, from its TXT record
    pub fw_version: Option<String>,
}

impl Device {
    /// Opens a link to the device.
    pub async fn connect(&self) -> Result<UdpLink, Error> {
        UdpLink::connect(self.addr).await
    }
}

/// Lists the RED devices advertising themselves on the local network.
///
/// Browses for [`udp::SERVICE_TYPE`] over mDNS, and returns every device that answers within
/// `timeout`, sorted by chip id. Needs multicast to reach the device, which some networks
/// (guest Wi-Fi, many corporate LANs) block; [`open`] can take an address instead.
pub async fn discover(timeout: Duration) -> Result<Vec<Device>, Error> {
    let daemon = ServiceDaemon::new()?;
    let events = daemon.browse(udp::SERVICE_TYPE)?;

    // Keyed by instance, as a device answering on several interfaces resolves more than once
    let mut found = HashMap::new();
    let collect = async {
        while let Ok(event) = events.recv_async().await {
            let ServiceEvent::ServiceResolved(service) = event else {
                continue;
            };

            // The device is IPv4 only, but prefer v4 regardless: a v6 link-local address
            // is no use without the scope that `SocketAddr` would need alongside it.
            let addrs: Vec<IpAddr> = service
                .get_addresses()
                .iter()
                .map(|a| a.to_ip_addr())
                .collect();
            let Some(ip) = addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()) else {
                debug!("ignoring {} with no address", service.get_fullname());
                continue;
            };

            let instance = instance_name(service.get_fullname());
            let chip_id = match service.get_property_val_str(udp::TXT_CHIP_ID) {
                Some(id) => id.to_string(),
                None => {
                    debug!("{} has no chip id, using its instance name", instance);
                    instance.clone()
                }
            };

            let device = Device {
                chip_id,
                addr: SocketAddr::new(*ip, service.get_port()),
                fw_version: service
                    .get_property_val_str(udp::TXT_FW_VERSION)
                    .map(str::to_string),
                instance: instance.clone(),
            };
            debug!("found {:?}", device);
            found.insert(instance, device);
        }
    };
    // Browsing never finishes by itself; the timeout is what ends it
    let _ = tokio::time::timeout(timeout, collect).await;

    // Shutting down only fails if the daemon is already gone, which is what we want anyway
    let _ = daemon.shutdown();

    let mut devices: Vec<Device> = found.into_values().collect();
    devices.sort_by(|a, b| a.chip_id.cmp(&b.chip_id));

    Ok(devices)
}

/// Opens a link to a RED device on the network.
///
/// `host` may be:
/// - an address, `192.168.1.20` or `192.168.1.20:7477`, which is used as is, with no mDNS,
/// - a chip id or instance name (`red-<chip id>`), which is looked up with [`discover`],
/// - nothing, which opens the only device discovered, failing if there are several.
pub async fn open(host: Option<&str>) -> Result<UdpLink, Error> {
    if let Some(addr) = host.and_then(parse_addr) {
        return UdpLink::connect(addr).await;
    }

    let mut devices = discover(DISCOVERY_TIMEOUT).await?;
    if let Some(host) = host {
        devices.retain(|d| d.chip_id == host || d.instance == host);
    }

    match devices.len() {
        0 => Err(Error::NoDevice),
        1 => devices[0].connect().await,
        _ => Err(Error::MultipleDevices(
            devices.into_iter().map(|d| d.chip_id).collect(),
        )),
    }
}

/// Parses `host` as a socket address, or as an IP address on the default port.
fn parse_addr(host: &str) -> Option<SocketAddr> {
    host.parse().ok().or_else(|| {
        host.parse::<IpAddr>()
            .ok()
            .map(|ip| SocketAddr::new(ip, udp::PORT))
    })
}

/// The instance label of a DNS-SD full name: `red-0123` from `red-0123._red._udp.local.`.
fn instance_name(fullname: &str) -> String {
    fullname
        .strip_suffix(udp::SERVICE_TYPE)
        .map(|s| s.trim_end_matches('.'))
        .unwrap_or(fullname)
        .to_string()
}

/// A [`Link`] to a device over UDP.
///
/// Each datagram carries exactly one COBS frame, so unlike [`StreamLink`](crate::StreamLink)
/// there is nothing to reassemble. UDP may drop, duplicate or reorder datagrams; the
/// [`Client`](crate::Client) already copes with each of those by matching ids and timing out.
pub struct UdpLink {
    socket: UdpSocket,
    buf: [u8; MAX_FRAME_LEN],
}

impl UdpLink {
    /// Opens a link to the device serving requests at `addr`.
    ///
    /// The socket is connected, so datagrams from anywhere but `addr` are discarded by the OS.
    pub async fn connect(addr: SocketAddr) -> Result<Self, Error> {
        let local = match addr {
            SocketAddr::V4(_) => SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0),
            SocketAddr::V6(_) => SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0),
        };

        let socket = UdpSocket::bind(local).await?;
        socket.connect(addr).await?;

        Ok(Self {
            socket,
            buf: [0; MAX_FRAME_LEN],
        })
    }

    /// The address of the device.
    pub fn peer_addr(&self) -> Result<SocketAddr, Error> {
        Ok(self.socket.peer_addr()?)
    }
}

impl Link for UdpLink {
    async fn send(&mut self, req: &Req) -> Result<(), Error> {
        let mut frame = [0u8; MAX_FRAME_LEN];
        let n = req.encode(&mut frame)?;

        self.socket.send(&frame[..n]).await?;

        Ok(())
    }

    async fn recv(&mut self) -> Result<Resp, Error> {
        loop {
            // A datagram longer than the buffer is truncated, and then fails to decode
            let n = self.socket.recv(&mut self.buf).await?;

            match Resp::decode(&mut self.buf[..n]) {
                Ok(resp) => return Ok(resp),
                Err(e) => warn!("dropped undecodable datagram: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use heapless::String;

    use red_proto::{
        req::ReqBody,
        resp::{DeviceInfo, RespBody},
    };

    use super::*;
    use crate::Client;

    /// A fake device on a loopback socket, answering each request with whatever `reply`
    /// returns. Returns its address.
    async fn device(reply: impl Fn(Req) -> Vec<Vec<u8>> + Send + 'static) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();

        tokio::spawn(async move {
            let mut buf = [0u8; MAX_FRAME_LEN];
            loop {
                let (n, from) = socket.recv_from(&mut buf).await.unwrap();
                let req = Req::decode(&mut buf[..n]).unwrap();
                for datagram in reply(req) {
                    socket.send_to(&datagram, from).await.unwrap();
                }
            }
        });

        addr
    }

    fn encode(resp: Resp) -> Vec<u8> {
        let mut buf = [0u8; MAX_FRAME_LEN];
        let n = resp.encode(&mut buf).unwrap();
        buf[..n].to_vec()
    }

    fn info(req: Req) -> Resp {
        Resp::new(
            req.id,
            RespBody::DeviceInfo(DeviceInfo {
                chip_id: String::try_from("0123").unwrap(),
                firmware_version: String::try_from("v1").unwrap(),
            }),
        )
    }

    async fn client(addr: SocketAddr) -> Client<UdpLink> {
        let link = UdpLink::connect(addr).await.unwrap();
        Client::new(link).with_timeout(Duration::from_millis(100))
    }

    #[tokio::test]
    async fn round_trips_a_request() {
        let addr = device(|req| vec![encode(info(req))]).await;

        let info = client(addr).await.device_info().await.unwrap();

        assert_eq!(info.chip_id, "0123");
    }

    #[tokio::test]
    async fn sends_one_frame_per_datagram() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut link = UdpLink::connect(socket.local_addr().unwrap())
            .await
            .unwrap();

        link.send(&Req::new(4, ReqBody::GetMeasurement))
            .await
            .unwrap();

        let mut buf = [0u8; 256];
        let n = socket.recv(&mut buf).await.unwrap();
        assert_eq!(buf[n - 1], 0, "a datagram ends with the frame's delimiter");
        assert_eq!(Req::decode(&mut buf[..n]).unwrap().id, 4);
    }

    #[tokio::test]
    async fn skips_undecodable_datagrams() {
        let addr = device(|req| vec![vec![0xde, 0xad, 0xbe, 0xef, 0x00], encode(info(req))]).await;

        assert!(client(addr).await.device_info().await.is_ok());
    }

    #[tokio::test]
    async fn times_out_without_a_response() {
        let addr = device(|_| vec![]).await;

        assert!(matches!(
            client(addr).await.device_info().await,
            Err(Error::Timeout)
        ));
    }

    #[tokio::test]
    async fn opens_an_address_without_discovery() {
        let addr = device(|req| vec![encode(info(req))]).await;

        let link = open(Some(&addr.to_string())).await.unwrap();

        assert_eq!(link.peer_addr().unwrap(), addr);
    }

    #[test]
    fn parses_addresses() {
        assert_eq!(
            parse_addr("10.0.0.1"),
            Some(SocketAddr::new([10, 0, 0, 1].into(), udp::PORT))
        );
        assert_eq!(
            parse_addr("10.0.0.1:99"),
            Some("10.0.0.1:99".parse().unwrap())
        );
        assert_eq!(
            parse_addr("::1"),
            Some(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), udp::PORT))
        );
        assert_eq!(parse_addr("red-0123"), None);
        assert_eq!(parse_addr("0123456789abcdef01234567"), None);
    }

    #[test]
    fn extracts_instance_names() {
        assert_eq!(instance_name("red-0123._red._udp.local."), "red-0123");
        assert_eq!(instance_name("elsewhere"), "elsewhere");
    }
}
