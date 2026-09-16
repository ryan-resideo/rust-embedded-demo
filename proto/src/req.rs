/// A request to the RED device, tagged with an identifier the matching
/// [`Resp`](crate::resp::Resp) echoes back so a source may have several in flight.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Req {
    /// Identifier for this request, chosen by the requester
    pub id: u32,
    /// The request to be handled
    pub body: ReqBody,
}

impl Req {
    /// Creates a request with the given identifier.
    pub const fn new(id: u32, body: ReqBody) -> Self {
        Self { id, body }
    }
}

/// Represents a request that can be sent to the RED device.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReqBody {
    /// Request device information
    GetDeviceInfo,
    /// Request a measurement from the device
    GetMeasurement,
}
