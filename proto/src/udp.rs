//! Constants the firmware's UDP interface and the host library must agree on.
//!
//! Each datagram carries exactly one COBS frame, a [`Req`](crate::req::Req) to the device or
//! a [`Resp`](crate::resp::Resp) back, so no reassembly is needed on either end.

/// UDP port the device serves requests on.
pub const PORT: u16 = 7477;

/// DNS-SD service type the device advertises over mDNS, without the `.local` domain.
pub const SERVICE: &str = "_red";

/// DNS-SD protocol of [`SERVICE`].
pub const PROTOCOL: &str = "_udp";

/// Fully qualified service type hosts browse for.
pub const SERVICE_TYPE: &str = "_red._udp.local.";

/// TXT record key carrying the device's chip id.
pub const TXT_CHIP_ID: &str = "chip_id";

/// TXT record key carrying the device's firmware version.
pub const TXT_FW_VERSION: &str = "fw";
