//! The media playback status channel (13).

use crate::channels::text;
use crate::consts::playback_msg;
use crate::log::{detail, hex};
use crate::wire::{decode_fields, decode_varint_value};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaPlaybackMetadata {
    pub song: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub playlist: Option<String>,
    pub duration_seconds: Option<u32>,
    pub rating: Option<u32>,
    /// JPEG or PNG as the phone sent it.
    pub album_art: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MediaPlaybackState {
    Stopped,
    Playing,
    Paused,
    #[default]
    Unknown,
}

impl MediaPlaybackState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaPlaybackStatus {
    pub state: MediaPlaybackState,
    pub media_source: Option<String>,
    pub playback_seconds: Option<u32>,
    pub shuffle: Option<bool>,
    pub repeat: Option<bool>,
    pub repeat_one: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaInfoEvent {
    Metadata(MediaPlaybackMetadata),
    Status(MediaPlaybackStatus),
}

fn opt<T: std::fmt::Debug>(v: &Option<T>) -> String {
    v.as_ref().map_or_else(|| "undefined".to_string(), |v| format!("{v:?}"))
}

#[derive(Debug, Default)]
pub struct MediaInfoChannel;

impl MediaInfoChannel {
    pub fn handle_message(&mut self, msg_id: u16, payload: &[u8]) -> Option<MediaInfoEvent> {
        match msg_id {
            playback_msg::METADATA => {
                let m = decode_metadata(payload);
                detail!(
                    "[MediaInfoChannel] metadata: song={} artist={} album={} duration={}s art={} playlist={}",
                    opt(&m.song),
                    opt(&m.artist),
                    opt(&m.album),
                    opt(&m.duration_seconds),
                    m.album_art.as_ref().map_or("none".to_string(), |a| format!("{}B", a.len())),
                    opt(&m.playlist)
                );
                Some(MediaInfoEvent::Metadata(m))
            }
            playback_msg::STATUS => {
                let s = decode_status(payload);
                detail!(
                    "[MediaInfoChannel] status: state={} pos={}s source={} shuffle={} repeat={}",
                    s.state.name(),
                    opt(&s.playback_seconds),
                    opt(&s.media_source),
                    opt(&s.shuffle),
                    opt(&s.repeat)
                );
                Some(MediaInfoEvent::Status(s))
            }
            playback_msg::INPUT => None,
            other => {
                let shown = hex(payload);
                detail!(
                    "[MediaInfoChannel] unhandled msgId=0x{other:x} len={} hex={}",
                    payload.len(),
                    &shown[..shown.len().min(80)]
                );
                None
            }
        }
    }
}

pub fn decode_metadata(payload: &[u8]) -> MediaPlaybackMetadata {
    let mut out = MediaPlaybackMetadata::default();
    for f in decode_fields(payload) {
        match f.field {
            1 => out.song = Some(text(f.bytes)),
            2 => out.artist = Some(text(f.bytes)),
            3 => out.album = Some(text(f.bytes)),
            4 => out.album_art = Some(f.bytes.to_vec()),
            5 => out.playlist = Some(text(f.bytes)),
            6 => out.duration_seconds = Some(decode_varint_value(f.bytes)),
            7 => out.rating = Some(decode_varint_value(f.bytes)),
            _ => {}
        }
    }
    out
}

pub fn decode_status(payload: &[u8]) -> MediaPlaybackStatus {
    let mut out = MediaPlaybackStatus::default();
    for f in decode_fields(payload) {
        match f.field {
            1 => {
                out.state = match decode_varint_value(f.bytes) {
                    1 => MediaPlaybackState::Stopped,
                    2 => MediaPlaybackState::Playing,
                    3 => MediaPlaybackState::Paused,
                    _ => MediaPlaybackState::Unknown,
                }
            }
            2 => out.media_source = Some(text(f.bytes)),
            3 => out.playback_seconds = Some(decode_varint_value(f.bytes)),
            4 => out.shuffle = Some(decode_varint_value(f.bytes) != 0),
            5 => out.repeat = Some(decode_varint_value(f.bytes) != 0),
            6 => out.repeat_one = Some(decode_varint_value(f.bytes) != 0),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_and_status_are_read_field_by_field() {
        let mut c = MediaInfoChannel;
        let meta = [0x0a, 0x01, b'S', 0x22, 0x02, 1, 2, 0x30, 0xb4, 0x01, 0x38, 0x05];
        assert_eq!(
            c.handle_message(playback_msg::METADATA, &meta),
            Some(MediaInfoEvent::Metadata(MediaPlaybackMetadata {
                song: Some("S".into()),
                album_art: Some(vec![1, 2]),
                duration_seconds: Some(180),
                rating: Some(5),
                ..Default::default()
            }))
        );
        let status = [0x08, 0x02, 0x12, 0x01, b'M', 0x18, 0x0a, 0x20, 0x01, 0x28, 0x00, 0x30, 0x01];
        assert_eq!(
            c.handle_message(playback_msg::STATUS, &status),
            Some(MediaInfoEvent::Status(MediaPlaybackStatus {
                state: MediaPlaybackState::Playing,
                media_source: Some("M".into()),
                playback_seconds: Some(10),
                shuffle: Some(true),
                repeat: Some(false),
                repeat_one: Some(true),
            }))
        );
        assert_eq!(decode_status(&[0x08, 0x09]).state, MediaPlaybackState::Unknown);
        assert_eq!(decode_status(&[0x08, 0x01]).state, MediaPlaybackState::Stopped);
        assert_eq!(decode_status(&[0x08, 0x03]).state, MediaPlaybackState::Paused);
        assert_eq!(
            decode_metadata(&[0x1a, 0x01, b'A', 0x12, 0x01, b'B', 0x2a, 0x01, b'P']).playlist,
            Some("P".into())
        );
        assert_eq!(c.handle_message(playback_msg::INPUT, &[]), None);
        assert_eq!(c.handle_message(0x9000, &[1; 60]), None);
    }
}
