//! The dongle's ports, the same on both ends of the USB link.

/// The MFi coprocessor.
pub const MFI: u16 = 5000;
/// Orders for the dongle, the Bluetooth accessory's with `accessory` in front.
pub const CONTROL: u16 = 5001;
/// The Bluetooth controller as raw HCI, for a host that drives it itself.
pub const HCI: u16 = 5002;
/// The iAP2 session the dongle's own Bluetooth hands over.
pub const IAP: u16 = 5003;
/// The accessory's orders, reachable on the dongle only.
pub const ACCESSORY: u16 = 5004;
