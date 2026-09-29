//! Constants the firmware's USB interface and the host library must agree on.

/// USB vendor ID.
///
/// `0xc0de`/`0xcafe` is the pair the embassy examples use. It is **not** an allocated ID and
/// is only suitable for a demo; a product needs its own (for example from pid.codes).
pub const VID: u16 = 0xc0de;

/// USB product ID. See [`VID`].
pub const PID: u16 = 0xcafe;

/// Interface class of the command interface: vendor specific.
pub const INTERFACE_CLASS: u8 = 0xff;

/// Interface subclass of the command interface.
pub const INTERFACE_SUBCLASS: u8 = 0x00;

/// Interface protocol of the command interface.
pub const INTERFACE_PROTOCOL: u8 = 0x00;

/// Maximum packet size of the bulk endpoints, fixed at 64 bytes for full speed.
pub const MAX_PACKET_SIZE: u16 = 64;
