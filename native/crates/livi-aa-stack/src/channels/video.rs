// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_aa_proto::{VideoFocusMode, VideoFocusNotification, VideoFocusRequest};
use prost::Message;

use crate::channels::Frame;
use crate::codec::encode;
use crate::log::{debug, detail};
use crate::wire::decode_start;
use livi_aa_proto::consts::{ch, frame_flags, media_msg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoChannelEvent {
    HostUiRequested,
    VideoFocusProjected,
}

#[derive(Debug)]
pub struct VideoChannel {
    channel: u8,
    session: u32,
}

impl VideoChannel {
    pub fn new(channel: u8) -> Self {
        Self {
            channel,
            session: 0,
        }
    }

    pub fn channel_id(&self) -> u8 {
        self.channel
    }

    fn label(&self) -> &'static str {
        if self.channel == ch::CLUSTER_VIDEO {
            "ClusterVideoChannel"
        } else {
            "VideoChannel"
        }
    }

    pub fn handle_message(
        &mut self,
        msg_id: u16,
        payload: &[u8],
    ) -> (Option<Frame>, Option<VideoChannelEvent>) {
        match msg_id {
            media_msg::START => {
                if let Some(start) = decode_start(payload) {
                    self.session = start.session_id;
                }
                detail!(
                    "[{}] stream started, session={}",
                    self.label(),
                    self.session
                );
                (None, None)
            }
            media_msg::STOP => {
                detail!("[{}] stream stopped", self.label());
                (None, None)
            }
            media_msg::VIDEO_FOCUS_NOTIFICATION => {
                if debug() {
                    println!("[{}] VideoFocusIndication", self.label());
                }
                (None, None)
            }
            media_msg::VIDEO_FOCUS_REQUEST => {
                let mode = focus_mode(payload);
                detail!(
                    "[{}] VideoFocusRequest mode={} -> responding PROJECTED",
                    self.label(),
                    mode.as_str_name()
                );
                let projected = VideoFocusNotification {
                    mode: Some(VideoFocusMode::Projected as i32),
                    unsolicited: None,
                };
                let answer = Frame::new(
                    self.channel,
                    frame_flags::ENC_SIGNAL,
                    media_msg::VIDEO_FOCUS_NOTIFICATION,
                    encode(&projected),
                );
                let event = match mode {
                    VideoFocusMode::Native | VideoFocusMode::NativeTransient => {
                        VideoChannelEvent::HostUiRequested
                    }
                    _ => VideoChannelEvent::VideoFocusProjected,
                };
                (Some(answer), Some(event))
            }
            other => {
                if debug() {
                    println!("[{}] unhandled msgId=0x{other:x}", self.label());
                }
                (None, None)
            }
        }
    }
}

fn focus_mode(payload: &[u8]) -> VideoFocusMode {
    VideoFocusRequest::decode(payload)
        .ok()
        .and_then(|r| r.mode)
        .and_then(|m| VideoFocusMode::try_from(m).ok())
        .unwrap_or(VideoFocusMode::Projected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_requests_are_answered_projected() {
        let mut v = VideoChannel::new(ch::VIDEO);
        let (answer, event) =
            v.handle_message(media_msg::VIDEO_FOCUS_REQUEST, &[0x08, 0x00, 0x10, 0x02]);
        assert_eq!(
            answer,
            Some(Frame::new(
                ch::VIDEO,
                frame_flags::ENC_SIGNAL,
                media_msg::VIDEO_FOCUS_NOTIFICATION,
                [8, 1]
            ))
        );
        assert_eq!(event, Some(VideoChannelEvent::HostUiRequested));
        let (_, event) = v.handle_message(media_msg::VIDEO_FOCUS_REQUEST, &[0x10, 0x03]);
        assert_eq!(event, Some(VideoChannelEvent::HostUiRequested));
        let (_, event) = v.handle_message(media_msg::VIDEO_FOCUS_REQUEST, &[]);
        assert_eq!(event, Some(VideoChannelEvent::VideoFocusProjected));
        let mut c = VideoChannel::new(ch::CLUSTER_VIDEO);
        let (answer, _) =
            c.handle_message(media_msg::VIDEO_FOCUS_REQUEST, &[0x10, 0x01, 0x18, 0x00]);
        assert_eq!(answer.unwrap().ch, ch::CLUSTER_VIDEO);
        assert_eq!(
            c.handle_message(media_msg::START, &[0x08, 0x04]),
            (None, None)
        );
        assert_eq!(c.session, 4);
        assert_eq!(c.handle_message(media_msg::STOP, &[]), (None, None));
        assert_eq!(
            c.handle_message(media_msg::VIDEO_FOCUS_NOTIFICATION, &[]),
            (None, None)
        );
        assert_eq!(c.handle_message(0x1234, &[]), (None, None));
        assert_eq!(c.channel_id(), ch::CLUSTER_VIDEO);
    }
}
