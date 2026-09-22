//! Transport and IPC plumbing shared by the helper's session-bearing drivers
//! (Android Auto, the dongle): the USB bulk byte stream, the per-session socket
//! the main process attaches to, the framed link protocol over it, and the
//! writer that feeds decoded media to the gst-video host.

pub mod feed;
pub mod link;
pub mod sock;
pub mod usb;
