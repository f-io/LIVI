// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! What the speaker plays now, told by the output whichever source it comes from.

use std::sync::Arc;

use livi_core_proto::state::AudioStream;
use livi_media::output::{AudioCodec, Output, OutputEvent};
use livi_output_proto::audio::AudioInfo;
use tokio::sync::broadcast::error::RecvError;

use crate::hub::Hub;

pub async fn follow(hub: Arc<Hub>, gst: Output) {
    let mut events = gst.subscribe();
    loop {
        match events.recv().await {
            Ok(OutputEvent::AudioHeard(info)) => {
                let audio = info.map(stream_of);
                hub.update(|s| s.audio = audio);
            }
            Err(RecvError::Closed) => return,
            _ => {}
        }
    }
}

fn stream_of(info: AudioInfo) -> AudioStream {
    use AudioCodec::*;
    let (codec, bits) = match info.codec {
        AacLc | AacRaw => ("AAC-LC", 0),
        AacEld => ("AAC-ELD", 0),
        Opus => ("Opus", 0),
        Alac => ("ALAC", 16),
        Alac24 => ("ALAC", 24),
        Pcm | PcmLe => ("LPCM", 16),
        Pcm24 => ("LPCM", 24),
    };
    AudioStream {
        codec: codec.into(),
        rate: info.rate,
        bits,
        channels: info.channels,
        buffered: info.realtime.map(|r| !r),
        buffer_ms: info.ahead_ms,
        target_ms: info.target_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lossy_codec_has_no_bit_depth() {
        let info = |codec| AudioInfo {
            codec,
            realtime: Some(false),
            rate: 48_000,
            channels: 2,
            ahead_ms: 190,
            target_ms: None,
        };
        assert_eq!(stream_of(info(AudioCodec::AacRaw)).bits, 0);
        assert_eq!(stream_of(info(AudioCodec::AacRaw)).codec, "AAC-LC");
        assert_eq!(stream_of(info(AudioCodec::Alac24)).bits, 24);
        assert_eq!(stream_of(info(AudioCodec::Pcm)).buffered, Some(true));
        let unnamed = AudioInfo {
            realtime: None,
            ..info(AudioCodec::Pcm)
        };
        assert_eq!(stream_of(unnamed).buffered, None);
    }
}
