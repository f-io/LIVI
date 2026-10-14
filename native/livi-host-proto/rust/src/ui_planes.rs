//! One connection per plane, framed like the control protocol with the plane id
//! as message id: a create, then its frames until the connection closes. A plane
//! that plays a URL instead gets PLAY_URL and its controls, and answers with STATUS.

/// The plane body, see `plane_body`.
pub const CREATE: u8 = 1;
/// One access unit.
pub const FRAME: u8 = 2;
/// Drops what is still queued from the feeder before.
pub const FLUSH: u8 = 3;
/// The URL body, see `url_body`.
pub const PLAY_URL: u8 = 4;
/// f64 LE, 0 pauses.
pub const PLAY_RATE: u8 = 5;
/// f64 LE seconds.
pub const PLAY_SEEK: u8 = 6;
/// 1 mutes.
pub const PLAY_MUTE: u8 = 7;
/// From the UI, see `url_status_body`.
pub const STATUS: u8 = 8;
/// From the UI, one log line of the player, which has no log of its own there.
pub const LOG: u8 = 9;

pub const PATH_ENV: &str = "LIVI_UI_PLANES";
