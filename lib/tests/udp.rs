//! Hardware-in-the-loop tests against a RED device on the local network.
//!
//! Ignored by default, as they need a board running the firmware with an Ethernet cable in
//! CN14, on the same LAN as this host. Run them with `make hitl-udp`, or
//! `cargo test -p red-lib --test udp -- --ignored`.
//!
//! Set `RED_HOST` to a chip id or an address to pick a device when several are on the
//! network, or to skip mDNS where multicast is blocked.

use std::time::Duration;

use red_lib::{Client, udp};

fn host() -> Option<String> {
    std::env::var("RED_HOST").ok()
}

async fn client() -> Client<udp::UdpLink> {
    let link = udp::open(host().as_deref()).await.unwrap();
    Client::new(link)
}

#[tokio::test]
#[ignore = "needs a RED device on the network"]
async fn udp_discovers_device() {
    let devices = udp::discover(udp::DISCOVERY_TIMEOUT).await.unwrap();
    assert!(!devices.is_empty(), "no devices advertised over mDNS");

    for device in devices {
        assert_eq!(device.chip_id.len(), 24);
        assert_eq!(device.instance, format!("red-{}", device.chip_id));
        assert_eq!(device.addr.port(), red_proto::udp::PORT);
        assert!(device.fw_version.is_some());
    }
}

#[tokio::test]
#[ignore = "needs a RED device on the network"]
async fn udp_device_info() {
    let mut client = client().await;

    let info = client.device_info().await.unwrap();

    // The chip id is the 96 bit UID in hex, and doubles as the mDNS instance name
    assert_eq!(info.chip_id.len(), 24);
    if let Some(host) = host().filter(|h| h.len() == 24) {
        assert_eq!(info.chip_id, host.as_str());
    }
    assert!(!info.firmware_version.is_empty());
}

#[tokio::test]
#[ignore = "needs a RED device on the network"]
async fn udp_measurement() {
    let mut client = client().await;

    // The engine measures every 10 s, so a freshly booted board may have nothing yet
    let measurement = loop {
        match client.measurement().await {
            Ok(m) => break m,
            Err(red_lib::Error::Device(_)) => tokio::time::sleep(Duration::from_secs(1)).await,
            Err(e) => panic!("measurement failed: {e}"),
        }
    };

    assert!(measurement.temperature > -40.0 && measurement.temperature < 85.0);
    assert!(measurement.pressure > 30_000.0 && measurement.pressure < 110_000.0);
    assert!(measurement.humidity >= 0.0 && measurement.humidity <= 100.0);
}
