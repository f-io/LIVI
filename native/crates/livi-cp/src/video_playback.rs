// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::time::Instant;

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;

use crate::bplist::{self, Value, dict};
use crate::media::VideoStatus;
use crate::opack;

pub const FEATURE: &str = "videoPlayback";
const ASSUMED_DURATION_S: f64 = 4.0 * 3600.0;
pub const DATASTREAM_UUID: &str = "A6B27562-B43A-4F2D-B75F-82391E250194";
pub const SETTINGS_DATASTREAM_UUID: &str = "BB493F61-A6B8-4769-8D74-80C23A9F71C4";
pub const OVERLAY_DATASTREAM_UUID: &str = "E3DC3EA6-E6C3-4B30-847C-B7ACFEBEA654";

pub fn features_ex(features: u64) -> String {
    let bytes = features.to_le_bytes();
    let len = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    STANDARD_NO_PAD.encode(&bytes[..len])
}

pub fn info_entries(features: u64, allowed: bool) -> [(String, Value); 2] {
    [
        (
            "videoPlaybackInfo".into(),
            dict([
                ("videoPlaybackAllowed", Value::Bool(allowed)),
                ("featuresEx", Value::String(features_ex(features))),
            ]),
        ),
        (
            "playbackCapabilities".into(),
            dict([
                ("supportsFPSSecureStop", Value::Bool(false)),
                ("supportsUIForAudioOnlyContent", Value::Bool(false)),
            ]),
        ),
    ]
}

pub fn allowed_command(allowed: bool, status: &str) -> Value {
    dict([
        ("type", Value::String("setVideoPlaybackAllowed".into())),
        (
            "params",
            dict([
                ("videoPlaybackAllowed", Value::Bool(allowed)),
                ("playbackStatus", Value::String(status.into())),
            ]),
        ),
    ])
}

const SCREEN: u64 = 1;
const BORROW: u64 = 3;
const UNBORROW: u64 = 4;
const USER_INITIATED: u64 = 500;

pub fn borrow_screen(borrow: bool) -> Value {
    let mut screen = vec![
        ("resourceID".to_string(), Value::Int(SCREEN)),
        (
            "transferType".to_string(),
            Value::Int(if borrow { BORROW } else { UNBORROW }),
        ),
        (
            "borrowID".to_string(),
            Value::String("VideoPlayback".into()),
        ),
    ];
    if borrow {
        screen.push(("transferPriority".to_string(), Value::Int(USER_INITIATED)));
        screen.push(("unborrowConstraint".to_string(), Value::Int(USER_INITIATED)));
    }
    dict([
        ("type", Value::String("changeModes".into())),
        (
            "params",
            dict([("resources", Value::Array(vec![Value::Dict(screen)]))]),
        ),
    ])
}

fn text_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn seconds(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    match v {
        Value::Real(f) => Some(*f),
        Value::Int(n) => Some(*n as f64),
        Value::Dict(_) => {
            let value = v.get("value").and_then(Value::as_int)? as f64;
            let scale = v
                .get("timescale")
                .and_then(Value::as_int)
                .filter(|s| *s > 0)? as f64;
            Some(value / scale)
        }
        _ => None,
    }
}

pub fn describe(message: &Value) -> String {
    let mut parts = Vec::new();
    for key in ["type", "controlMessage", "kind", "property"] {
        if let Some(t) = text_of(message, key) {
            parts.push(format!("{key}={t}"));
        }
    }
    if let Some(id) = message.get("messageID").and_then(Value::as_int) {
        parts.push(format!("messageID={id}"));
    }
    if let Some(url) = message
        .get("item")
        .and_then(|i| text_of(i, "Content-Location"))
    {
        parts.push(format!("url={url}"));
    }
    if let Some(rate) = seconds(message.get("rate")) {
        parts.push(format!("rate={rate}"));
    }
    if let Some(time) = seconds(message.get("time")) {
        parts.push(format!("time={time:.3}"));
    }
    match message.get("value") {
        Some(Value::Bool(on)) => parts.push(format!("value={on}")),
        Some(info @ Value::Dict(_)) => {
            for key in ["position", "rate"] {
                if let Some(v) = seconds(info.get(key)) {
                    parts.push(format!("{key}={v:.3}"));
                }
            }
        }
        _ => {}
    }
    if parts.is_empty() {
        "no type".into()
    } else {
        parts.join(" ")
    }
}

#[derive(Default)]
pub struct Playback {
    item: Option<String>,
    start_s: f64,
    rate: f64,
    since: Option<Instant>,
}

impl Playback {
    pub fn position(&self) -> f64 {
        let ran = self.since.map_or(0.0, |t| t.elapsed().as_secs_f64());
        (self.start_s + ran * self.rate).min(ASSUMED_DURATION_S)
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    pub fn handle(
        &mut self,
        message: &Value,
        status: Option<&VideoStatus>,
    ) -> (Option<Value>, bool) {
        let mut started = false;
        match text_of(message, "type") {
            Some("insertPlayQueueItem") => {
                let item = message.get("item");
                self.item = item.and_then(|i| text_of(i, "uuid")).map(str::to_string);
                self.start_s = seconds(item.and_then(|i| i.get("Start-Position"))).unwrap_or(0.0);
                self.since = None;
            }
            Some("setRate") => {
                let rate = seconds(message.get("rate")).unwrap_or(0.0);
                self.start_s = self.position();
                started = rate > 0.0 && self.rate == 0.0;
                self.rate = rate;
                self.since = Some(Instant::now());
            }
            Some("seek") => {
                if let Some(to) = seconds(message.get("time")) {
                    self.start_s = to;
                    self.since = self.since.map(|_| Instant::now());
                }
            }
            Some("stop") => *self = Self::default(),
            _ => {}
        }
        if text_of(message, "kind") != Some("request") {
            return (None, started);
        }
        let mut answer = vec![("kind".to_string(), Value::String("response".into()))];
        for key in ["type", "controlMessage", "property"] {
            if let Some(v) = message.get(key) {
                answer.push((key.to_string(), v.clone()));
            }
        }
        if let Some(id) = message.get("messageID") {
            answer.push(("messageID".to_string(), id.clone()));
        }
        if text_of(message, "type") == Some("playbackInfo") {
            answer.push(("value".to_string(), self.info(status)));
        }
        (Some(Value::Dict(answer)), started)
    }

    fn info(&self, status: Option<&VideoStatus>) -> Value {
        let s = status.copied().unwrap_or_default();
        let duration = s
            .duration
            .filter(|d| *d > 0.0)
            .unwrap_or(ASSUMED_DURATION_S);
        let position = if s.ended {
            duration
        } else {
            s.position.unwrap_or(self.start_s)
        };
        let ready = s.ready && !s.failed;
        let running = s.playing && !s.ended && !s.failed;
        let (empty, full) = match s.buffered_percent {
            Some(percent) => (percent == 0, percent >= 100),
            None => (!ready, ready),
        };
        let (start, end) = s.seekable.unwrap_or((0.0, duration));
        let range = || {
            Value::Array(vec![dict([
                ("start", Value::Real(start)),
                ("duration", Value::Real((end - start).max(0.0))),
            ])])
        };
        let mut info = vec![
            ("duration".to_string(), Value::Real(duration)),
            ("position".to_string(), Value::Real(position)),
            (
                "rate".to_string(),
                Value::Real(if running { self.rate } else { 0.0 }),
            ),
            ("readyToPlay".to_string(), Value::Bool(ready)),
            ("playbackBufferEmpty".to_string(), Value::Bool(empty)),
            ("playbackBufferFull".to_string(), Value::Bool(full)),
            (
                "playbackLikelyToKeepUp".to_string(),
                Value::Bool(ready && !empty),
            ),
            ("loadedTimeRanges".to_string(), range()),
            ("seekableTimeRanges".to_string(), range()),
        ];
        if let Some(item) = &self.item {
            info.push(("uuid".to_string(), Value::String(item.clone())));
        }
        Value::Dict(info)
    }
}

pub fn describe_setting(message: &[u8]) -> String {
    let Some(v) = opack::decode(message) else {
        return "not OPACK".into();
    };
    let kind = v.get("messageType").and_then(opack::Value::as_int);
    let data = match v.get("data") {
        Some(opack::Value::Data(d)) => match bplist::decode(d) {
            Ok(p) => format!("{p:?}"),
            Err(_) => format!("{} B", d.len()),
        },
        Some(other) => format!("{other:?}"),
        None => "none".into(),
    };
    format!(
        "messageType={} data={data}",
        kind.map_or("?".into(), |k| k.to_string())
    )
}

pub struct Capture {
    dir: PathBuf,
    kept: u32,
}

impl Capture {
    pub fn new(root: &Path) -> Self {
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Self {
            dir: root.join(started.to_string()),
            kept: 0,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn keep(&mut self, source: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        self.kept += 1;
        let path = self.dir.join(format!("{:04}-{source}.bin", self.kept));
        std::fs::write(&path, data)?;
        if let Ok(v) = bplist::decode(data) {
            std::fs::write(path.with_extension("txt"), format!("{v:#?}\n"))?;
        } else if opack::decode(data).is_some() {
            std::fs::write(path.with_extension("txt"), describe_setting(data) + "\n")?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_ex_is_the_feature_bits_without_trailing_zeros() {
        assert_eq!(
            features_ex(0x61_5653_AEE2),
            STANDARD_NO_PAD.encode([0xE2, 0xAE, 0x53, 0x56, 0x61])
        );
        assert_eq!(features_ex(0), "");
    }

    #[test]
    fn info_follows_the_vehicle_and_claims_no_fairplay() {
        let [(info, allowed), (caps, playback)] = info_entries(0x80, true);
        assert_eq!(
            (info.as_str(), caps.as_str()),
            ("videoPlaybackInfo", "playbackCapabilities")
        );
        assert_eq!(
            allowed.get("videoPlaybackAllowed").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            allowed.get("featuresEx").and_then(Value::as_str),
            Some("gA")
        );
        assert_eq!(
            playback
                .get("supportsFPSSecureStop")
                .and_then(Value::as_bool),
            Some(false)
        );
        let [(_, driving), _] = info_entries(0x80, false);
        assert_eq!(
            driving.get("videoPlaybackAllowed").and_then(Value::as_bool),
            Some(false)
        );
        let cmd = allowed_command(false, "idle");
        assert_eq!(
            cmd.get("type").and_then(Value::as_str),
            Some("setVideoPlaybackAllowed")
        );
        let params = cmd.get("params").unwrap();
        assert_eq!(
            params.get("playbackStatus").and_then(Value::as_str),
            Some("idle")
        );
        assert_eq!(
            params.get("videoPlaybackAllowed").and_then(Value::as_bool),
            Some(false)
        );
    }

    fn message(entries: Vec<(&str, Value)>) -> Value {
        Value::Dict(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    #[test]
    fn a_request_is_answered_like_a_playing_receiver() {
        let mut playback = Playback::default();
        let start = dict([
            ("value", Value::Int(60_000)),
            ("timescale", Value::Int(1000)),
        ]);
        let item = dict([
            ("uuid", Value::String("item-1".into())),
            ("Start-Position", start),
            ("Content-Location", Value::String("https://x/a.m3u8".into())),
        ]);
        let insert = message(vec![
            ("type", Value::String("insertPlayQueueItem".into())),
            ("item", item),
        ]);
        assert_eq!(
            describe(&insert),
            "type=insertPlayQueueItem url=https://x/a.m3u8"
        );
        assert_eq!(playback.handle(&insert, None), (None, false));

        let rate = message(vec![
            ("type", Value::String("setRate".into())),
            ("rate", Value::Int(1)),
        ]);
        assert_eq!(describe(&rate), "type=setRate rate=1");
        let mute = message(vec![
            ("type", Value::String("setProperty".into())),
            ("property", Value::String("muted".into())),
            ("value", Value::Bool(true)),
        ]);
        assert_eq!(
            describe(&mute),
            "type=setProperty property=muted value=true"
        );
        assert_eq!(playback.handle(&rate, None), (None, true));
        assert_eq!(
            playback.handle(&rate, None),
            (None, false),
            "only the first start counts"
        );

        let ask = message(vec![
            ("type", Value::String("playbackInfo".into())),
            ("kind", Value::String("request".into())),
            ("messageID", Value::Int(7)),
        ]);
        let playing = VideoStatus {
            position: Some(61.5),
            ready: true,
            playing: true,
            ..Default::default()
        };
        let (answer, _) = playback.handle(&ask, Some(&playing));
        let answer = answer.unwrap();
        assert_eq!(
            describe(&answer),
            "type=playbackInfo kind=response messageID=7 position=61.500 rate=1.000"
        );
        let info = answer.get("value").unwrap();
        assert_eq!(
            info.get("position").and_then(|p| seconds(Some(p))),
            Some(61.5)
        );
        assert_eq!(info.get("rate").and_then(|p| seconds(Some(p))), Some(1.0));
        assert_eq!(info.get("readyToPlay").and_then(Value::as_bool), Some(true));
        assert!(
            (60.0..61.0).contains(&playback.position()),
            "the phone's own clock"
        );
        assert_eq!(info.get("uuid").and_then(Value::as_str), Some("item-1"));

        let other = message(vec![
            ("type", Value::String("somethingNew".into())),
            ("kind", Value::String("request".into())),
            ("messageID", Value::Int(8)),
        ]);
        let (answer, _) = playback.handle(&other, None);
        assert!(answer.unwrap().get("value").is_none());

        let to = dict([
            ("value", Value::Int(600_000_000)),
            ("timescale", Value::Int(1_000_000)),
        ]);
        let seek = message(vec![
            ("type", Value::String("seek".into())),
            ("kind", Value::String("request".into())),
            ("messageID", Value::Int(9)),
            ("time", to),
        ]);
        playback.handle(&seek, None);
        assert!((600.0..601.0).contains(&playback.position()));

        let stop = message(vec![("type", Value::String("stop".into()))]);
        playback.handle(&stop, None);
        assert_eq!(playback.position(), 0.0);
    }

    fn info_for(status: Option<VideoStatus>) -> Value {
        let mut playback = Playback::default();
        let rate = message(vec![
            ("type", Value::String("setRate".into())),
            ("rate", Value::Int(1)),
        ]);
        playback.handle(&rate, None);
        let ask = message(vec![
            ("type", Value::String("playbackInfo".into())),
            ("kind", Value::String("request".into())),
        ]);
        playback
            .handle(&ask, status.as_ref())
            .0
            .unwrap()
            .get("value")
            .unwrap()
            .clone()
    }

    fn real(v: &Value, key: &str) -> f64 {
        seconds(v.get(key)).unwrap()
    }

    fn flag(v: &Value, key: &str) -> bool {
        v.get(key).and_then(Value::as_bool).unwrap()
    }

    fn range(v: &Value, key: &str) -> (f64, f64) {
        let r = &v.get(key).and_then(Value::as_array).unwrap()[0];
        (real(r, "start"), real(r, "duration"))
    }

    #[test]
    fn the_answer_tells_what_the_player_says() {
        let silent = info_for(None);
        assert!(!flag(&silent, "readyToPlay") && flag(&silent, "playbackBufferEmpty"));
        assert_eq!(real(&silent, "rate"), 0.0);

        let filling = VideoStatus {
            ready: true,
            playing: true,
            buffered_percent: Some(40),
            ..Default::default()
        };
        let filling = info_for(Some(filling));
        assert!(!flag(&filling, "playbackBufferEmpty") && !flag(&filling, "playbackBufferFull"));
        assert!(flag(&filling, "playbackLikelyToKeepUp"));
        let dry = info_for(Some(VideoStatus {
            ready: true,
            buffered_percent: Some(0),
            ..Default::default()
        }));
        assert!(!flag(&dry, "playbackLikelyToKeepUp"));

        let live = VideoStatus {
            position: Some(30.0),
            seekable: Some((20.0, 35.0)),
            ready: true,
            playing: true,
            ..Default::default()
        };
        let live = info_for(Some(live));
        assert_eq!(real(&live, "duration"), ASSUMED_DURATION_S);
        assert_eq!(range(&live, "seekableTimeRanges"), (20.0, 15.0));

        let film = VideoStatus {
            position: Some(50.0),
            duration: Some(600.0),
            ready: true,
            ended: true,
            ..Default::default()
        };
        let film = info_for(Some(film));
        assert_eq!((real(&film, "position"), real(&film, "rate")), (600.0, 0.0));
        assert_eq!(range(&film, "loadedTimeRanges"), (0.0, 600.0));

        let broken = VideoStatus {
            ready: true,
            playing: true,
            failed: true,
            ..Default::default()
        };
        let broken = info_for(Some(broken));
        assert!(!flag(&broken, "readyToPlay"));
        assert_eq!(real(&broken, "rate"), 0.0);
    }

    #[test]
    fn the_screen_is_borrowed_for_video_and_given_back() {
        let borrow = borrow_screen(true);
        assert_eq!(
            borrow.get("type").and_then(Value::as_str),
            Some("changeModes")
        );
        let take = &borrow
            .get("params")
            .and_then(|p| p.get("resources"))
            .unwrap();
        let screen = &take.as_array().unwrap()[0];
        assert_eq!(
            screen.get("transferType").and_then(Value::as_int),
            Some(BORROW)
        );
        assert_eq!(
            screen.get("borrowID").and_then(Value::as_str),
            Some("VideoPlayback")
        );
        assert_eq!(
            screen.get("unborrowConstraint").and_then(Value::as_int),
            Some(USER_INITIATED)
        );
        let give = borrow_screen(false);
        let screen = &give
            .get("params")
            .and_then(|p| p.get("resources"))
            .unwrap()
            .as_array()
            .unwrap()[0];
        assert_eq!(
            screen.get("transferType").and_then(Value::as_int),
            Some(UNBORROW)
        );
        assert!(screen.get("transferPriority").is_none());
    }

    #[test]
    fn opack_reads_the_settings_message() {
        let inner = bplist::encode(&dict([("property", Value::String("p".into()))]));
        let mut msg = vec![0xE2, 0x44];
        msg.extend_from_slice(b"data");
        msg.push(0x91);
        msg.push(inner.len() as u8);
        msg.extend_from_slice(&inner);
        msg.push(0x4B);
        msg.extend_from_slice(b"messageType");
        msg.push(0x0D);
        let v = opack::decode(&msg).unwrap();
        assert_eq!(v.get("messageType").and_then(opack::Value::as_int), Some(5));
        let line = describe_setting(&msg);
        assert!(
            line.starts_with("messageType=5 data=") && line.contains("\"p\""),
            "{line}"
        );

        let open = [0xDF, 0x01, 0x30, 0xFF, 0x03, 0xEF, 0x41, b'k', 0x02, 0x03];
        assert_eq!(
            opack::decode(&open[..5]),
            Some(opack::Value::Array(vec![
                opack::Value::Bool(true),
                opack::Value::Int(-1)
            ]))
        );
        let dict_end = opack::decode(&open[5..]).unwrap();
        assert_eq!(dict_end.get("k"), Some(&opack::Value::Bool(false)));
        assert_eq!(describe_setting(b"\xA0"), "not OPACK");
    }

    #[test]
    fn a_payload_is_kept_with_its_plist_text() {
        let root = std::env::temp_dir().join(format!("livi-video-{}", std::process::id()));
        let mut capture = Capture::new(&root);
        let plist = bplist::encode(&dict([("url", Value::String("https://x".into()))]));
        let first = capture.keep("stream2", &plist).unwrap();
        let second = capture.keep("settings", &[0xE1, 0x41, b'k', 0x09]).unwrap();
        let third = capture.keep("settings", b"\xA0raw").unwrap();
        assert!(first.ends_with("0001-stream2.bin") && second.ends_with("0002-settings.bin"));
        assert_eq!(std::fs::read(&first).unwrap(), plist);
        assert!(
            std::fs::read_to_string(first.with_extension("txt"))
                .unwrap()
                .contains("https://x")
        );
        assert!(
            std::fs::read_to_string(second.with_extension("txt"))
                .unwrap()
                .contains("data=")
        );
        assert!(!third.with_extension("txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
