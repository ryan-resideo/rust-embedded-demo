//! Hardware-in-the-loop tests against a RED device attached over USB.
//!
//! Ignored by default, as they need a board running the firmware with CN13 (user USB)
//! connected. Run them with `make hitl-usb`, or `cargo test -p red-lib --test usb -- --ignored`.
//! Set `RED_SERIAL` to pick a device when several are attached.

use red_lib::{Client, usb};
use tokio::sync::{Mutex, MutexGuard, OnceCell};

fn serial() -> Option<String> {
    std::env::var("RED_SERIAL").ok()
}

// Only one process can hold the USB interface, so all tests share one lazily-opened
// connection, serialized by this lock rather than each racing to claim it.
static CLIENT: OnceCell<Mutex<Client<usb::UsbLink>>> = OnceCell::const_new();

async fn client() -> MutexGuard<'static, Client<usb::UsbLink>> {
    CLIENT
        .get_or_init(|| async {
            let link = usb::open(serial().as_deref()).await.unwrap();
            Mutex::new(Client::new(link))
        })
        .await
        .lock()
        .await
}

#[tokio::test]
#[ignore = "needs a RED device attached over USB"]
async fn usb_device_info() {
    let serial = serial();
    let mut client = client().await;

    let info = client.device_info().await.unwrap();

    // The chip id is the 96 bit UID in hex, and doubles as the USB serial number
    assert_eq!(info.chip_id.len(), 24);
    if let Some(serial) = serial {
        assert_eq!(info.chip_id, serial.as_str());
    }
    assert!(!info.firmware_version.is_empty());
}

#[tokio::test]
#[ignore = "needs a RED device attached over USB"]
async fn usb_measurement() {
    let mut client = client().await;

    // The engine measures every 10 s, so a freshly booted board may have nothing yet
    let measurement = loop {
        match client.measurement().await {
            Ok(m) => break m,
            Err(red_lib::Error::Device(_)) => {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await
            }
            Err(e) => panic!("measurement failed: {e}"),
        }
    };

    assert!(measurement.temperature > -40.0 && measurement.temperature < 85.0);
    assert!(measurement.pressure > 30_000.0 && measurement.pressure < 110_000.0);
    assert!(measurement.humidity >= 0.0 && measurement.humidity <= 100.0);
}
