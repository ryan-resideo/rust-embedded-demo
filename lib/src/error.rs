use crate::Transport;

/// Errors raised while talking to a RED device.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The USB stack reported an error
    #[error("usb: {0}")]
    Usb(#[from] nusb::Error),

    /// Reading from or writing to the link failed
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// A request could not be encoded
    #[error("encode: {0}")]
    Encode(#[from] postcard::Error),

    /// The link closed
    #[error("device disconnected")]
    Disconnected,

    /// No response arrived in time
    #[error("timed out waiting for a response")]
    Timeout,

    /// No matching device is attached
    #[error("no RED device found")]
    NoDevice,

    /// More than one device matched, and none was picked out
    #[error("multiple RED devices found, pick one by serial number: {}", .0.join(", "))]
    MultipleDevices(Vec<String>),

    /// The device does not expose the command interface
    #[error("device has no RED command interface")]
    NoInterface,

    /// The device could not service the request
    #[error("device error: {0:?}")]
    Device(red_proto::resp::Error),

    /// The device answered with a response of the wrong kind
    #[error("unexpected response from device")]
    UnexpectedResponse,

    /// The transport is not implemented yet
    #[error("transport {0:?} is not yet supported")]
    Unsupported(Transport),
}
