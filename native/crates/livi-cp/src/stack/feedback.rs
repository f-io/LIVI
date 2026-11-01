// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<M: Media> Conn<M> {
    pub(super) fn audio_started(&mut self, stream: u32, first_sample: u32) {
        let Some(a) = self.audio.iter_mut().find(|a| a.host_stream == stream) else {
            return;
        };
        a.origin = Some((first_sample, Instant::now()));
        let profile = a.profile.clone();
        let mut mic_on = None;
        if a.mic.is_none()
            && let Some((key, req)) = a.mic_plan.clone()
        {
            let (rate, channels) = (req.sample_rate, req.channels);
            a.mic = Some(self.ctx.media.open_mic(key, req));
            mic_on = Some((rate, channels));
        }
        self.emit(StackEvent::AudioActive {
            profile,
            active: true,
        });
        if let Some((rate, channels)) = mic_on {
            self.emit(StackEvent::MicActive {
                active: true,
                rate,
                channels,
            });
        }
    }

    pub(super) fn build_feedback(&self) -> Response {
        if self.audio.is_empty() {
            return status(200);
        }
        let streams = self
            .audio
            .iter()
            .map(|a| {
                let mut s = vec![
                    ("type".to_string(), Value::Int(a.stream_type)),
                    (
                        "sampleRate".to_string(),
                        Value::Int(u64::from(a.sample_rate)),
                    ),
                ];
                if let Some((first, at)) = a.origin {
                    let output = self
                        .ctx
                        .media
                        .audio_output_delay(a.host_stream)
                        .unwrap_or(0);
                    let play = sounding_sample(
                        first,
                        at.elapsed().as_secs_f64(),
                        a.playout_latency_ms + output,
                        a.sample_rate,
                    );
                    let ntp = self
                        .timing
                        .as_ref()
                        .map_or_else(ntp64_now, TimingSync::synced_ntp);
                    s.push(("streamConnectionID".to_string(), a.connection_id.clone()));
                    s.push(("timestamp".to_string(), Value::Int(ntp)));
                    s.push(("timestampRawNs".to_string(), Value::Int(mono_ns())));
                    s.push(("sampleTime".to_string(), Value::Int(u64::from(play))));
                }
                Value::Dict(s)
            })
            .collect();
        plist_response(&dict([("streams", Value::Array(streams))]))
    }
}
