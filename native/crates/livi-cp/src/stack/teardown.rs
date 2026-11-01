// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<M: Media> Conn<M> {
    pub(super) fn handle_teardown(&mut self, req: &Request) -> Response {
        let types: Vec<u64> = bplist::decode(&req.body)
            .ok()
            .and_then(|b| {
                b.get("streams")
                    .and_then(Value::as_array)
                    .map(|s| s.iter().map(|s| int(s.get("type"))).collect())
            })
            .unwrap_or_default();
        if types.is_empty() {
            println!("[cpStack] TEARDOWN (session)");
            self.teardown();
            return status(200);
        }
        let list: Vec<String> = types.iter().map(u64::to_string).collect();
        println!("[cpStack] TEARDOWN streams {}", list.join(","));
        for kind in types {
            if kind == STREAM_MAIN_SCREEN || kind == STREAM_ALT_SCREEN {
                self.close_screen(kind == STREAM_ALT_SCREEN);
                continue;
            }
            if let Some(i) = self.audio.iter().position(|a| a.stream_type == kind) {
                let a = self.audio.remove(i);
                self.close_audio(a);
            }
        }
        status(200)
    }

    pub(super) fn close_screen(&mut self, cluster: bool) {
        let slot = if cluster {
            &mut self.cluster_screen
        } else {
            &mut self.screen
        };
        if let Some(receiver) = slot.take() {
            self.ctx.media.close_screen(receiver);
        }
    }

    pub(super) fn close_audio(&mut self, a: AudioStreamState) {
        self.ctx.media.close_audio(a.host_stream);
        self.emit(StackEvent::AudioActive {
            profile: a.profile.clone(),
            active: false,
        });
        if a.profile.kind == AudioKind::Call {
            self.call_active = false;
        }
        if let Some(mic) = a.mic {
            self.ctx.media.close_mic(mic);
            self.emit(StackEvent::MicActive {
                active: false,
                rate: 0,
                channels: 0,
            });
        }
    }

    pub(super) fn teardown(&mut self) {
        self.timing = None;
        self.keep_alive = None;
        self.close_screen(false);
        self.close_screen(true);
        for a in std::mem::take(&mut self.audio) {
            self.close_audio(a);
        }
        self.iap_tunnel = None;
        self.iap_out = None;
        self.iap_relay = None;
        self.event_listener = None;
    }
}
