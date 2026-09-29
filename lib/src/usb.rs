//! Connecting to a device's USB command interface.

use nusb::{
    DeviceInfo,
    descriptors::TransferType,
    io::{EndpointRead, EndpointWrite},
    transfer::{Bulk, Direction, In, Out},
};

use red_proto::usb;

use crate::{Error, StreamLink};

/// A [`Link`](crate::Link) to a device over USB.
pub type UsbLink = StreamLink<EndpointRead<Bulk>, EndpointWrite<Bulk>>;

/// Lists the attached RED devices.
pub async fn list() -> Result<Vec<DeviceInfo>, Error> {
    let devices = nusb::list_devices()
        .await?
        .filter(|d| d.vendor_id() == usb::VID && d.product_id() == usb::PID)
        .collect();

    Ok(devices)
}

/// Opens the command interface of an attached RED device.
///
/// With `serial`, opens the device with that USB serial number (its chip id). Without it,
/// opens the only device attached, failing if there are several.
pub async fn open(serial: Option<&str>) -> Result<UsbLink, Error> {
    let mut devices = list().await?;
    if let Some(serial) = serial {
        devices.retain(|d| d.serial_number() == Some(serial));
    }

    let info = match devices.len() {
        0 => return Err(Error::NoDevice),
        1 => devices.remove(0),
        _ => {
            let serials = devices
                .iter()
                .map(|d| d.serial_number().unwrap_or("<none>").to_string())
                .collect();
            return Err(Error::MultipleDevices(serials));
        }
    };

    let device = info.open().await?;

    // Find the interface by its class rather than trusting fixed numbers and addresses
    let config = device.active_configuration().map_err(nusb::Error::from)?;
    let (number, ep_in, ep_out) = config
        .interface_alt_settings()
        .filter(|alt| {
            alt.class() == usb::INTERFACE_CLASS
                && alt.subclass() == usb::INTERFACE_SUBCLASS
                && alt.protocol() == usb::INTERFACE_PROTOCOL
        })
        .find_map(|alt| {
            let bulk = |direction| {
                alt.endpoints()
                    .find(|ep| {
                        ep.transfer_type() == TransferType::Bulk && ep.direction() == direction
                    })
                    .map(|ep| ep.address())
            };
            Some((
                alt.interface_number(),
                bulk(Direction::In)?,
                bulk(Direction::Out)?,
            ))
        })
        .ok_or(Error::NoInterface)?;

    let interface = device.claim_interface(number).await?;

    let packet = usb::MAX_PACKET_SIZE as usize;
    let reader = interface.endpoint::<Bulk, In>(ep_in)?.reader(packet);
    let writer = interface.endpoint::<Bulk, Out>(ep_out)?.writer(packet);

    Ok(StreamLink::new(reader, writer))
}
