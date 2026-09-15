use heapless::String;

/// A response from the RED device, carrying the identifier of the
/// [`Req`](crate::req::Req) that produced it.
#[derive(Debug, Clone)]
pub struct Resp {
    /// Identifier of the originating request
    pub id: u32,
    /// The response to that request
    pub body: RespBody,
}

impl Resp {
    /// Creates a response against the given request identifier.
    pub const fn new(id: u32, body: RespBody) -> Self {
        Self { id, body }
    }
}

/// Represents a response received from the RED device.
#[derive(Debug, Clone)]
pub enum RespBody {
    /// Information about the device
    DeviceInfo(DeviceInfo),
    /// The current status of the device
    Measurement(Measurement),
}

/// Represents device information returned by the RED device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// The unique identifier of the chip.
    pub chip_id: String<32>,
    /// The firmware version running on the device.
    pub firmware_version: String<32>,
}

/// Represents a measurement returned by the RED device.
#[derive(Debug, Clone)]
pub struct Measurement {
    pub temperature: f32,
    pub pressure: f32,
    pub humidity: f32,
}
