// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use livi_core_proto::features::{self, *};
use livi_core_proto::helper::{MIRROR_NAME, Mirror};
use livi_core_proto::helper_events::NowPlayingPatch;
use livi_cp::airplay::{
    AirplayCodec, AirplayCommand, AirplayCtx, AirplayFormat, AirplayMedia, AirplayMeta,
    AirplayRemote, AirplaySender, serve,
};
use livi_cp::helper_sock::HelperSock;
use livi_cp::identity::Identity;
use livi_cp::pairings::Pairings;
use livi_media::output::{AudioCodec, AudioOpts, Output};
use livi_media::planes::MainPlane;
use livi_output_proto::PLANE_AIRPLAY;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Notify, mpsc};

use crate::hub::Hub;
use crate::projection::ApEvent;

const TAG: &str = "[airplay]";
const FEATURES: u64 = VIDEO
    | PHOTO
    | VIDEO_FAIRPLAY
    | VIDEO_HLS
    | SLIDESHOW
    | UNKNOWN_6
    | SCREEN
    | SCREEN_ROTATE
    | AUDIO
    | UNKNOWN_10
    | AUDIO_REDUNDANT
    | PHOTO_CACHING
    | META_ARTWORK
    | META_PROGRESS
    | META_TEXT
    | AUDIO_FORMAT_1
    | AUDIO_FORMAT_2
    | AUDIO_FORMAT_3
    | AUDIO_FORMAT_4
    | AUDIO_UNENCRYPTED
    | ITUNES4W_ENCRYPTION
    | AUDIO_AES_MFI
    | UNKNOWN_28
    | RAOP
    | VIDEO_PLAY_QUEUE
    | FROM_CLOUD
    | TLS_PSK
    | UNKNOWN_36
    | COREUTILS_PAIRING
    | BUFFERED_AUDIO
    | PTP
    | HOMEKIT_PAIRING
    | PEER_MANAGEMENT
    | TRANSIENT_PAIRING
    | META_PLIST
    | HANGDOG_REMOTE
    | AUDIO_MEDIA_DATA_CONTROL;

pub struct Receiver {
    listener: TcpListener,
    port: u16,
    device_id: String,
    source_version: String,
    txt: Vec<String>,
}

impl Receiver {
    pub fn mirror(&self) -> Mirror {
        Mirror {
            port: self.port,
            txt: self.txt.clone(),
        }
    }
}

pub fn bind(identity: &Identity, accessory: &str, source_version: String) -> Option<Receiver> {
    let listener = match livi_cp::net::tcp_listener() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{TAG} cannot listen: {e}");
            return None;
        }
    };
    let port = livi_cp::net::local_port(listener.local_addr());
    let device_id = airplay_id(accessory);
    let txt = vec![
        format!("deviceid={device_id}"),
        format!("features={}", features::txt(FEATURES)),
        "flags=0x4".to_string(),
        "model=LIVI".to_string(),
        format!("srcvers={source_version}"),
        "protovers=1.1".to_string(),
        "acl=0".to_string(),
        format!("pi={}", identity.pairing_id),
        format!("pk={}", identity.pk_hex()),
    ];
    println!("{TAG} listening on :{port} as {device_id}");
    Some(Receiver {
        listener,
        port,
        device_id,
        source_version,
        txt,
    })
}

pub fn start(
    receiver: Receiver,
    user_data: &Path,
    identity: &Identity,
    helper: &Path,
    gst: Output,
    airplay: Airplay,
) {
    let config = airplay.hub.config();
    let told = config.hu_volume;
    let sample_rate = if config.sampling_frequency == 1 {
        48_000
    } else {
        44_100
    };
    let media = Arc::new(GstAirplay {
        gst,
        plane: airplay.plane,
        receiver: Mutex::new(None),
        audio: airplay.audio,
        events: airplay.events,
        hub: airplay.hub,
        settings: airplay.settings,
        remotes: Mutex::new(HashMap::new()),
        told: Mutex::new(told),
    });
    tokio::spawn(follow_volume(media.clone()));
    let ctx = Arc::new(AirplayCtx {
        media,
        identity: identity.clone(),
        pairings: Pairings::new(user_data.join("cp/airplay-pairings.json")),
        helper: HelperSock::new(helper),
        device_id: receiver.device_id,
        name: MIRROR_NAME.into(),
        source_version: receiver.source_version,
        features: FEATURES,
        txt: receiver.txt,
        sample_rate,
    });
    tokio::spawn(serve(receiver.listener, ctx));
}

pub struct Airplay {
    pub plane: Arc<MainPlane>,
    pub audio: Arc<ApAudio>,
    pub events: mpsc::UnboundedSender<ApEvent>,
    pub hub: Arc<Hub>,
    pub settings: mpsc::UnboundedSender<Value>,
}

struct GstAirplay {
    gst: Output,
    plane: Arc<MainPlane>,
    receiver: Mutex<Option<u32>>,
    audio: Arc<ApAudio>,
    events: mpsc::UnboundedSender<ApEvent>,
    hub: Arc<Hub>,
    settings: mpsc::UnboundedSender<Value>,
    remotes: Mutex<HashMap<u64, AirplayRemote>>,
    told: Mutex<f64>,
}

async fn follow_volume(media: Arc<GstAirplay>) {
    let mut state = media.hub.watch();
    while state.changed().await.is_ok() {
        let level = state.borrow_and_update().config.hu_volume;
        {
            let mut told = lock(&media.told);
            if (level - *told).abs() < SAME_LEVEL {
                continue;
            }
            *told = level;
        }
        for remote in lock(&media.remotes).values() {
            remote.send(AirplayCommand::Volume(level));
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

const QUIETEST_DB: f64 = -30.0;
const MUTED_DB: f64 = -144.0;
const SAME_LEVEL: f64 = 0.005;

fn level_of(db: f64) -> f64 {
    if db <= QUIETEST_DB {
        0.0
    } else {
        ((db - QUIETEST_DB) / -QUIETEST_DB).min(1.0)
    }
}

fn db_of(level: f64) -> f64 {
    if level <= 0.0 {
        MUTED_DB
    } else {
        QUIETEST_DB * (1.0 - level.min(1.0))
    }
}

pub struct ApAudio {
    gst: Output,
    stream: Mutex<Option<u32>>,
    duck: Mutex<f64>,
}

impl ApAudio {
    pub fn new(gst: Output) -> Arc<Self> {
        Arc::new(Self {
            gst,
            stream: Mutex::new(None),
            duck: Mutex::new(1.0),
        })
    }

    fn opened(&self, stream: u32) {
        *self.stream.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream);
        let level = *self.duck.lock().unwrap_or_else(|e| e.into_inner());
        if level < 1.0 {
            self.gst.set_audio_volume(stream, level, 0);
        }
    }

    fn take(&self) -> Option<u32> {
        take(&self.stream)
    }

    pub fn duck(&self, level: f64, ramp_ms: u32) {
        *self.duck.lock().unwrap_or_else(|e| e.into_inner()) = level;
        if let Some(stream) = *self.stream.lock().unwrap_or_else(|e| e.into_inner()) {
            self.gst.set_audio_volume(stream, level, ramp_ms);
        }
    }
}

fn take(slot: &Mutex<Option<u32>>) -> Option<u32> {
    slot.lock().unwrap_or_else(|e| e.into_inner()).take()
}

impl AirplayMedia for GstAirplay {
    fn open_screen(&self, key: [u8; 32]) -> Pin<Box<dyn Future<Output = u16> + Send + '_>> {
        Box::pin(async move {
            self.close_screen();
            let (port, receiver) = self
                .gst
                .open_video_receiver(PLANE_AIRPLAY, &key, false)
                .await;
            self.gst.set_active_feeder(receiver, true);
            *self.receiver.lock().unwrap_or_else(|e| e.into_inner()) = Some(receiver);
            port
        })
    }

    fn close_screen(&self) {
        if let Some(receiver) = take(&self.receiver) {
            self.gst.close_video_receiver(receiver);
        }
        self.plane.dispose();
    }

    fn open_audio(
        &self,
        key: [u8; 32],
        format: AirplayFormat,
    ) -> Pin<Box<dyn Future<Output = (u16, u16)> + Send + '_>> {
        Box::pin(async move {
            self.close_audio();
            let opts = AudioOpts {
                codec: match (format.codec, format.bits) {
                    (AirplayCodec::AacEld, _) => AudioCodec::AacEld,
                    (AirplayCodec::Alac, 24) => AudioCodec::Alac24,
                    (AirplayCodec::Alac, _) => AudioCodec::Alac,
                },
                payload_type: 96,
                clock_rate: format.rate,
                channels: 2,
                latency_ms: 100,
                realtime: true,
                fed: false,
                device: String::new(),
            };
            let (stream, data, control) = self.gst.open_audio(&key, &opts).await;
            self.gst.set_audio_active(stream, true);
            self.audio.opened(stream);
            (data, control)
        })
    }

    fn close_audio(&self) {
        if let Some(stream) = self.audio.take() {
            self.gst.close_audio(stream);
        }
    }

    fn started(&self, id: u64, close: Arc<Notify>, peer: AirplaySender, remote: AirplayRemote) {
        lock(&self.remotes).insert(id, remote.clone());
        let _ = self.events.send(ApEvent::Started {
            id,
            close,
            peer,
            remote,
        });
    }

    fn ended(&self, id: u64) {
        lock(&self.remotes).remove(&id);
        let _ = self.events.send(ApEvent::Ended { id });
    }

    fn volume(&self, db: f64) {
        let level = level_of(db);
        *lock(&self.told) = level;
        if (level - self.hub.config().hu_volume).abs() < SAME_LEVEL {
            return;
        }
        let _ = self.settings.send(json!({ "huVolume": level }));
    }

    fn volume_db(&self) -> f64 {
        db_of(self.hub.config().hu_volume)
    }

    fn metadata(&self, id: u64, meta: AirplayMeta) {
        let patch = now_playing(meta);
        let _ = self.events.send(ApEvent::NowPlaying { id, patch });
    }
}

fn now_playing(meta: AirplayMeta) -> NowPlayingPatch {
    match meta {
        AirplayMeta::Track {
            title,
            artist,
            album,
        } => NowPlayingPatch {
            title,
            artist,
            album,
            app: Some("AirPlay".into()),
            ..Default::default()
        },
        AirplayMeta::Artwork(bytes) => NowPlayingPatch {
            artwork: Some(STANDARD.encode(bytes)),
            ..Default::default()
        },
        AirplayMeta::Progress {
            elapsed_ms,
            duration_ms,
        } => NowPlayingPatch {
            elapsed_ms: Some(elapsed_ms),
            duration_ms: Some(duration_ms),
            ..Default::default()
        },
        AirplayMeta::Playing(playing) => NowPlayingPatch {
            playing: Some(playing),
            ..Default::default()
        },
    }
}

fn airplay_id(accessory: &str) -> String {
    let mut octets: Vec<String> = accessory.split(':').map(str::to_string).collect();
    if let Some(last) = octets.last_mut()
        && let Ok(v) = u8::from_str_radix(last, 16)
    {
        *last = format!("{:02X}", v ^ 0x01);
    }
    octets.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_are_announced_as_a_low_and_a_high_word() {
        assert_eq!(features::txt(FEATURES), "0x567FAFF7,0x1405C35E");
    }

    #[test]
    fn the_airplay_slider_maps_linearly_onto_the_head_unit_volume() {
        assert_eq!(level_of(0.0), 1.0);
        assert_eq!(level_of(-15.0), 0.5);
        assert_eq!(level_of(-30.0), 0.0);
        assert_eq!(level_of(-144.0), 0.0);
        assert_eq!(db_of(0.5), -15.0);
        assert_eq!(db_of(0.0), -144.0);
        assert_eq!(db_of(1.0), 0.0);
    }

    #[test]
    fn airplay_differs_from_the_accessory_in_the_last_octet() {
        assert_eq!(airplay_id("00:50:43:02:FC:01"), "00:50:43:02:FC:00");
    }
}
