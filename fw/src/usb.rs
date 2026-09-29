//! USB transport: a vendor specific interface with one bulk OUT and one bulk IN endpoint.
//!
//! Requests and responses travel as COBS frames (see `red_proto`), so a frame may span
//! several packets, or share one with the next, without the USB layer knowing or caring.
//! The interface identifiers the host looks for live in [`red_proto::usb`].

use defmt::{debug, info, warn};
use embassy_futures::select::{Either, select};
use embassy_usb::driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::msos::{self, windows_version};
use embassy_usb::{Builder, Config, UsbDevice};
use postcard::accumulator::{CobsAccumulator, FeedResult};
use static_cell::ConstStaticCell;

use red_proto::{MAX_FRAME_LEN, req::Req, usb};

use crate::channels::{Port, Response};

/// The interface GUID Windows registers the device under once it has bound WinUSB via the
/// MS OS 2.0 descriptors. Generated for this project; host libraries enumerate by it.
const DEVICE_INTERFACE_GUIDS: &[&str] = &["{A0EEA927-F590-4309-B0AC-F000D2375FDE}"];

/// USB has a single peer, the host, so every request uses the same endpoint.
const ENDPOINT: u16 = 0;

const PACKET_LEN: usize = usb::MAX_PACKET_SIZE as usize;

static CONFIG_DESCRIPTOR: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);
static BOS_DESCRIPTOR: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);
static MSOS_DESCRIPTOR: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);
static CONTROL_BUF: ConstStaticCell<[u8; 64]> = ConstStaticCell::new([0; 64]);

/// The bulk endpoints of the command interface.
pub struct UsbTransport<D: Driver<'static>> {
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
}

impl<D: Driver<'static>> UsbTransport<D> {
    /// Describes the device and its command interface to `embassy_usb`.
    ///
    /// Returns the [`UsbDevice`], which must be run to service enumeration and control
    /// requests, and the transport, which services the command interface.
    ///
    /// `serial` is reported as the USB serial number, so hosts can pick a device out by it.
    ///
    /// Panics if called twice: the descriptor buffers are static.
    pub fn new(driver: D, serial: &'static str) -> (UsbDevice<'static, D>, Self) {
        let mut config = Config::new(usb::VID, usb::PID);
        config.manufacturer = Some("Rust Embedded Demo");
        config.product = Some("RED sensor");
        config.serial_number = Some(serial);
        config.self_powered = true;
        // A single function needs no interface association descriptors, so the class
        // is declared per interface rather than as a composite device.
        config.composite_with_iads = false;
        config.device_class = 0x00;
        config.device_sub_class = 0x00;
        config.device_protocol = 0x00;

        let mut builder = Builder::new(
            driver,
            config,
            CONFIG_DESCRIPTOR.take(),
            BOS_DESCRIPTOR.take(),
            MSOS_DESCRIPTOR.take(),
            CONTROL_BUF.take(),
        );

        // Tells Windows (8.1+) to bind its built-in WinUSB driver, so no INF or driver
        // install is needed for a userspace library to claim the interface.
        builder.msos_descriptor(windows_version::WIN8_1, 0);
        builder.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
        builder.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
            "DeviceInterfaceGUIDs",
            msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
        ));

        let mut function = builder.function(
            usb::INTERFACE_CLASS,
            usb::INTERFACE_SUBCLASS,
            usb::INTERFACE_PROTOCOL,
        );
        let mut interface = function.interface();
        let mut alt = interface.alt_setting(
            usb::INTERFACE_CLASS,
            usb::INTERFACE_SUBCLASS,
            usb::INTERFACE_PROTOCOL,
            None,
        );
        let read_ep = alt.endpoint_bulk_out(None, usb::MAX_PACKET_SIZE);
        let write_ep = alt.endpoint_bulk_in(None, usb::MAX_PACKET_SIZE);
        drop(function);

        (builder.build(), Self { read_ep, write_ep })
    }

    /// Services the host's requests through `port`, across any number of connections.
    pub async fn run(&mut self, port: Port) -> ! {
        loop {
            // Enabled once the host has configured the device
            self.read_ep.wait_enabled().await;
            info!("usb: host connected");

            // Responses to a previous connection's requests have nowhere to go
            while port.try_receive().is_ok() {}

            let Self { read_ep, write_ep } = self;
            let (Either::First(e) | Either::Second(e)) =
                select(receive(read_ep, &port), transmit(write_ep, &port)).await;

            info!("usb: host disconnected: {}", e);
        }
    }
}

/// Reassembles requests from OUT packets and submits them to the engine, until the endpoint
/// is disabled.
async fn receive<E: EndpointOut>(ep: &mut E, port: &Port) -> EndpointError {
    let mut packet = [0u8; PACKET_LEN];
    let mut frames = CobsAccumulator::<MAX_FRAME_LEN>::new();

    loop {
        let n = match ep.read(&mut packet).await {
            Ok(n) => n,
            Err(e) => return e,
        };

        let mut window = &packet[..n];
        while !window.is_empty() {
            // A bad frame ends at its delimiter, so the accumulator resyncs on the next one
            window = match frames.feed::<Req>(window) {
                FeedResult::Consumed => break,
                FeedResult::OverFull(rest) => {
                    warn!("usb: dropped oversized frame");
                    rest
                }
                FeedResult::DeserError(rest) => {
                    warn!("usb: dropped undecodable frame");
                    rest
                }
                FeedResult::Success { data, remaining } => {
                    debug!("usb: request {}", data);
                    port.request(ENDPOINT, data).await;
                    remaining
                }
            };
        }
    }
}

/// Encodes the engine's responses and writes them to the IN endpoint, until the endpoint is
/// disabled.
async fn transmit<E: EndpointIn>(ep: &mut E, port: &Port) -> EndpointError {
    let mut frame = [0u8; MAX_FRAME_LEN];

    loop {
        let Response { resp, .. } = port.receive().await;

        let n = match resp.encode(&mut frame) {
            Ok(n) => n,
            Err(_) => {
                warn!("usb: failed to encode response {}", resp.id);
                continue;
            }
        };

        for chunk in frame[..n].chunks(PACKET_LEN) {
            if let Err(e) = ep.write(chunk).await {
                return e;
            }
        }

        // A transfer ends on a short packet, so a frame that fills its last packet is
        // followed by a zero-length one for hosts reading in larger transfers.
        if n % PACKET_LEN == 0
            && let Err(e) = ep.write(&[]).await
        {
            return e;
        }
    }
}
