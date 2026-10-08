//! Recorded output of the TypeScript Android Auto stack. This stack has to send
//! the same bytes and report the same things.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use livi_aa_proto::ServiceDiscoveryRequest;
use serde_json::{Map, Value as Json, json};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};

use crate::aa_session::{self, SessionCmd};
use crate::bridge::{Bridge, BridgeOut, Navigation, NowPlaying, Report};
use crate::channels::audio::AudioChannelType;
use crate::channels::input::{self, TouchPointer};
use crate::channels::media_info::{
    MediaInfoChannel, MediaInfoEvent, MediaPlaybackMetadata, MediaPlaybackStatus,
};
use crate::channels::nav_maneuver::{
    nav_maneuver_type_to_code, nav_maneuver_type_to_side, turn_event_to_maneuver_type,
    turn_side_to_navi_code,
};
use crate::channels::navigation::{
    NavigationChannel, NavigationEvent, NavigationTurnEvent, NavigationTurnSide,
};
use crate::channels::{Emit, Frame};
use crate::commands::{Command, InputCommand, TouchAction, TouchItem};
use crate::config::{AaConfig, Insets, android_auto_dpi, fitting_tier};
use crate::config::{Addresses, Codecs, Geometry, from_livi};
use crate::control::{ControlChannel, ControlEvent};
use crate::discovery;
use crate::link::{self, Link, LinkItem, LinkReader};
use crate::log::hex;
use crate::media::AudioKind;
use crate::media::tests::FakeMedia;
use crate::sensors::{GpsFix, Sensor};
use crate::session::{Out, Session, SessionEvent, TEST_WALL, Timer};
use crate::wire::{decode_fields, decode_start, encode_varint, field_float, read_varint};

const T0: u64 = 1_700_000_000_000;

fn vectors() -> &'static Json {
    static V: OnceLock<Json> = OnceLock::new();
    V.get_or_init(|| {
        serde_json::from_str(include_str!("ts_vectors.json")).expect("valid vector file")
    })
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n =
            chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn u(v: &Json) -> u32 {
    v.as_u64().unwrap_or(0) as u32
}

fn opt_u(v: &Json, k: &str) -> Option<u32> {
    v.get(k).and_then(Json::as_u64).map(|n| n as u32)
}

fn put<T: Into<Json>>(o: &mut Map<String, Json>, k: &str, v: Option<T>) {
    if let Some(v) = v {
        o.insert(k.into(), v.into());
    }
}

fn ts_config(v: &Json) -> AaConfig {
    let n = |k: &str| v.get(k).map_or(0, u);
    let s = |k: &str| v.get(k).and_then(Json::as_str).map(str::to_string);
    let b = |k: &str| v.get(k).and_then(Json::as_bool).unwrap_or(false);
    let list = |k: &str| -> Vec<i32> {
        v.get(k)
            .and_then(Json::as_array)
            .map(|a| a.iter().map(|x| x.as_i64().unwrap() as i32).collect())
            .unwrap_or_default()
    };
    let insets = |prefix: &str| Insets {
        top: n(&format!("{prefix}Top")),
        bottom: n(&format!("{prefix}Bottom")),
        left: n(&format!("{prefix}Left")),
        right: n(&format!("{prefix}Right")),
    };
    AaConfig {
        hu_name: s("huName"),
        video_width: opt_u(v, "videoWidth"),
        video_height: opt_u(v, "videoHeight"),
        video_dpi: opt_u(v, "videoDpi"),
        video_fps: opt_u(v, "videoFps"),
        pixel_aspect_ratio_e4: opt_u(v, "pixelAspectRatioE4"),
        display_width: n("displayWidth"),
        display_height: n("displayHeight"),
        main_view_area: insets("mainViewArea"),
        main_safe_area: insets("mainSafeArea"),
        driver_position: n("driverPosition") as u8,
        bt_mac_address: s("btMacAddress"),
        wifi_bssid: s("wifiBssid"),
        wifi_ssid: s("wifiSsid").unwrap_or_default(),
        wifi_password: s("wifiPassword").unwrap_or_default(),
        wifi_security: Default::default(),
        wifi_channel: opt_u(v, "wifiChannel"),
        fuel_types: list("fuelTypes"),
        ev_connector_types: list("evConnectorTypes"),
        hevc_supported: b("hevcSupported"),
        vp9_supported: b("vp9Supported"),
        av1_supported: b("av1Supported"),
        initial_night_mode: v.get("initialNightMode").and_then(Json::as_bool),
        cluster_enabled: b("clusterEnabled"),
        cluster_width: n("clusterWidth"),
        cluster_height: n("clusterHeight"),
        cluster_tier_width: opt_u(v, "clusterTierWidth"),
        cluster_tier_height: opt_u(v, "clusterTierHeight"),
        cluster_pixel_aspect_ratio_e4: opt_u(v, "clusterPixelAspectRatioE4"),
        cluster_fps: n("clusterFps"),
        cluster_dpi: opt_u(v, "clusterDpi"),
        cluster_view_area: insets("clusterViewArea"),
        cluster_safe_area: insets("clusterSafeArea"),
        disable_audio_output: b("disableAudioOutput"),
        mic_device: String::new(),
        telephony_audio: false,
    }
}

fn frame_json(f: &Frame) -> Json {
    json!([f.ch, f.flags, f.msg_id, hex(&f.payload)])
}

#[test]
fn wire_helpers_match() {
    let v = vectors();
    for case in v["varints"].as_array().unwrap() {
        let n = case[0].as_f64().unwrap() as i64;
        assert_eq!(hex(&encode_varint(n)), case[1], "varint {n}");
    }
    for case in v["floats"].as_array().unwrap() {
        assert_eq!(hex(&field_float(1, case[0].as_f64().unwrap())), case[1], "float {}", case[0]);
    }
    for case in v["readVarint"].as_array().unwrap() {
        let (value, n) = read_varint(&unhex(case[0].as_str().unwrap()), 0);
        assert_eq!(json!([value, n]), case[1], "readVarint {}", case[0]);
    }
    for case in v["decodeStart"].as_array().unwrap() {
        let got = decode_start(&unhex(case[0].as_str().unwrap())).map(|s| {
            json!({ "sessionId": s.session_id, "configIndex": s.config_index.map_or(-1, i64::from) })
        });
        assert_eq!(got.unwrap_or(Json::Null), case[1], "decodeStart {}", case[0]);
    }
    for case in v["decodeFields"].as_array().unwrap() {
        let bytes = unhex(case[0].as_str().unwrap());
        let got: Vec<Json> =
            decode_fields(&bytes).map(|f| json!([f.field, f.wire, hex(f.bytes)])).collect();
        assert_eq!(Json::from(got), case[1], "decodeFields {}", case[0]);
    }
}

#[test]
fn service_discovery_matches() {
    for case in vectors()["sdr"].as_array().unwrap() {
        let d = discovery::build(&ts_config(&case["config"]));
        let name = &case["name"];
        assert_eq!(hex(&d.buf), case["hex"], "sdr {name}");
        let names = |c: &[discovery::VideoCodec]| {
            Json::from(c.iter().map(|c| c.name()).collect::<Vec<_>>())
        };
        assert_eq!(names(&d.video_codecs), case["video"], "sdr {name} video");
        assert_eq!(names(&d.cluster_codecs), case["cluster"], "sdr {name} cluster");
    }
}

fn sdr_request_json(r: &ServiceDiscoveryRequest) -> Json {
    let mut o = Map::new();
    put(&mut o, "deviceName", r.head_unit_label.clone());
    put(&mut o, "deviceBrand", r.phone_make_and_model.clone());
    if let Some(p) = &r.mobile_device_identity {
        let mut pi = Map::new();
        put(&mut pi, "instanceId", p.mobile_device_id.clone());
        put(&mut pi, "connectivityLifetimeId", p.connectivity_lifetime_id.clone());
        o.insert("phoneInfo".into(), Json::Object(pi));
    }
    Json::Object(o)
}

fn control_event_json(e: &ControlEvent) -> Json {
    match e {
        ControlEvent::ServiceDiscoveryRequest(r) => {
            json!(["service-discovery-request", sdr_request_json(r)])
        }
        ControlEvent::ChannelOpenRequest(id) => json!(["channel-open-request", id]),
        ControlEvent::AvSetupRequest { ch, payload } => {
            json!(["av-setup-request", ch, hex(payload)])
        }
        ControlEvent::Ping(ts) => json!(["ping", ts]),
        ControlEvent::Pong => json!(["pong"]),
        ControlEvent::AudioFocusRequest(t) => json!(["audio-focus-request", t]),
        ControlEvent::Battery { level, critical, time_remaining_s } => {
            let mut o = Map::new();
            put(&mut o, "level", *level);
            o.insert("critical".into(), json!(critical));
            put(&mut o, "timeRemaining", *time_remaining_s);
            json!(["battery", o])
        }
        ControlEvent::Shutdown(r) => json!(["shutdown", r]),
        ControlEvent::ShutdownComplete => json!(["shutdown-complete"]),
        ControlEvent::VoiceSession(a) => json!(["voice-session", a]),
    }
}

#[test]
fn control_channel_matches() {
    for case in vectors()["control"].as_array().unwrap() {
        let msg_id = case["msgId"].as_u64().unwrap() as u16;
        let mut c = ControlChannel;
        let out = c.handle_message(msg_id, &unhex(case["payload"].as_str().unwrap()));
        let mut sent = Vec::new();
        let mut events = Vec::new();
        for o in &out {
            match o {
                Emit::Send(f) => sent.push(frame_json(f)),
                Emit::Event(e) => events.push(control_event_json(e)),
            }
        }
        let label = format!("control 0x{msg_id:04x} {}", case["payload"]);
        assert_eq!(Json::from(sent), case["sent"], "{label} sent");
        assert_eq!(Json::from(events), case["events"], "{label} events");
    }
}

fn metadata_json(m: &MediaPlaybackMetadata) -> Json {
    let mut o = Map::new();
    put(&mut o, "song", m.song.clone());
    put(&mut o, "artist", m.artist.clone());
    put(&mut o, "album", m.album.clone());
    put(&mut o, "albumArt", m.album_art.as_deref().map(hex));
    put(&mut o, "playlist", m.playlist.clone());
    put(&mut o, "durationSeconds", m.duration_seconds);
    put(&mut o, "rating", m.rating);
    Json::Object(o)
}

fn status_json(s: &MediaPlaybackStatus) -> Json {
    let mut o = Map::new();
    o.insert("state".into(), json!(s.state.name()));
    put(&mut o, "mediaSource", s.media_source.clone());
    put(&mut o, "playbackSeconds", s.playback_seconds);
    put(&mut o, "shuffle", s.shuffle);
    put(&mut o, "repeat", s.repeat);
    put(&mut o, "repeatOne", s.repeat_one);
    Json::Object(o)
}

fn nav_event_json(e: &NavigationEvent) -> Json {
    match e {
        NavigationEvent::Start => json!(["nav-start"]),
        NavigationEvent::Stop => json!(["nav-stop"]),
        NavigationEvent::Status(s) => json!(["nav-status", { "state": s.name() }]),
        NavigationEvent::Turn(t) => {
            let mut o = Map::new();
            put(&mut o, "road", t.road.clone());
            put(&mut o, "turnSide", t.turn_side.map(NavigationTurnSide::name));
            put(&mut o, "event", t.event.map(NavigationTurnEvent::name));
            put(&mut o, "image", t.image.as_deref().map(hex));
            put(&mut o, "turnNumber", t.turn_number);
            put(&mut o, "turnAngle", t.turn_angle);
            json!(["nav-turn", o])
        }
        NavigationEvent::Distance(d) => {
            let mut o = Map::new();
            o.insert("distanceMeters".into(), json!(d.distance_meters));
            o.insert("timeToTurnSeconds".into(), json!(d.time_to_turn_seconds));
            put(&mut o, "displayDistanceE3", d.display_distance_e3);
            put(&mut o, "displayUnit", d.display_unit);
            json!(["nav-distance", o])
        }
        NavigationEvent::State(s) => {
            let mut o = Map::new();
            put(&mut o, "maneuverType", s.maneuver_type);
            put(&mut o, "roadName", s.road_name.clone());
            put(&mut o, "cue", s.cue.clone());
            put(&mut o, "destinationAddress", s.destination_address.clone());
            json!(["nav-state", o])
        }
        NavigationEvent::Position(p) => {
            let mut o = Map::new();
            put(&mut o, "stepDistanceMeters", p.step_distance_meters);
            put(&mut o, "stepDistanceDisplay", p.step_distance_display.clone());
            put(&mut o, "timeToStepSeconds", p.time_to_step_seconds);
            put(&mut o, "destinationMeters", p.destination_meters);
            put(&mut o, "destinationDisplay", p.destination_display.clone());
            put(&mut o, "destinationUnits", p.destination_units);
            put(&mut o, "etaText", p.eta_text.clone());
            put(&mut o, "timeToArrivalSeconds", p.time_to_arrival_seconds);
            put(&mut o, "currentRoadName", p.current_road_name.clone());
            json!(["nav-position", o])
        }
    }
}

#[test]
fn channel_decoders_match() {
    let v = vectors();
    let mut media = MediaInfoChannel;
    for case in v["mediaMeta"].as_array().unwrap() {
        let Some(MediaInfoEvent::Metadata(m)) =
            media.handle_message(0x8003, &unhex(case[0].as_str().unwrap()))
        else {
            panic!("no metadata")
        };
        assert_eq!(metadata_json(&m), case[1], "metadata {}", case[0]);
    }
    for case in v["mediaStatus"].as_array().unwrap() {
        let Some(MediaInfoEvent::Status(s)) =
            media.handle_message(0x8001, &unhex(case[0].as_str().unwrap()))
        else {
            panic!("no status")
        };
        assert_eq!(status_json(&s), case[1], "status {}", case[0]);
    }
    let mut nav = NavigationChannel;
    for case in v["navigation"].as_array().unwrap() {
        let id = case["msgId"].as_u64().unwrap() as u16;
        let got: Vec<Json> = nav
            .handle_message(id, &unhex(case["payload"].as_str().unwrap()))
            .iter()
            .map(nav_event_json)
            .collect();
        assert_eq!(Json::from(got), case["events"], "navigation 0x{id:04x} {}", case["payload"]);
    }
}

fn side_of(v: &Json) -> Option<NavigationTurnSide> {
    match v.as_str() {
        Some("left") => Some(NavigationTurnSide::Left),
        Some("right") => Some(NavigationTurnSide::Right),
        Some("unspecified") => Some(NavigationTurnSide::Unspecified),
        _ => None,
    }
}

#[test]
fn maneuver_maps_match() {
    let v = vectors();
    for case in v["turnMap"].as_array().unwrap() {
        let event = NavigationTurnEvent::ALL
            .into_iter()
            .find(|e| e.name() == case[0])
            .expect("known event");
        let got = turn_event_to_maneuver_type(Some(event), side_of(&case[1])).map(|m| m as u8);
        assert_eq!(json!(got), case[2], "turn {} {}", case[0], case[1]);
    }
    for case in v["sideMap"].as_array().unwrap() {
        assert_eq!(json!(turn_side_to_navi_code(side_of(&case[0])).map(|s| s as u8)), case[1]);
    }
    for case in v["maneuverMap"].as_array().unwrap() {
        let t = Some(u(&case[0]));
        assert_eq!(json!(nav_maneuver_type_to_code(t).map(|m| m as u8)), case[1], "maneuver {t:?}");
        assert_eq!(json!(nav_maneuver_type_to_side(t).map(|s| s as u8)), case[2], "side {t:?}");
    }
    for case in v["tiers"].as_array().unwrap() {
        let (w, h) = fitting_tier(u(&case[0]), u(&case[1]), case[2].as_bool().unwrap());
        assert_eq!(json!([w, h]), json!([case[3], case[4]]), "tier {case}");
    }
    for case in v["dpi"].as_array().unwrap() {
        assert_eq!(json!(android_auto_dpi(u(&case[0]), u(&case[1]))), case[2], "dpi {case}");
    }
}

#[test]
fn input_reports_match() {
    let ts = T0 * 1000;
    let p = |x, y, id| TouchPointer { x, y, id };
    let sent: Vec<Json> = [
        input::touch(ts, 0, &[p(10, 20, 0)], 0),
        input::touch(ts, 5, &[p(100, 200, 0), p(1279, 719, 1)], 1),
        input::touch(ts, 2, &[], 0),
        input::button(ts, &[3], true, false),
        input::button(ts, &[21, 260], false, true),
        input::button(ts, &[], true, false),
        Some(input::rotary(ts, -1)),
        Some(input::rotary(ts, 1)),
    ]
    .iter()
    .flatten()
    .map(frame_json)
    .collect();
    assert_eq!(Json::from(sent), vectors()["input"]);
}

fn set_wall(ms: u64) {
    TEST_WALL.with(|w| w.set(Some(ms)));
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Due {
    Ping,
    Timer(Timer),
}

struct Replay {
    s: Session,
    _tx: watch::Sender<AaConfig>,
    now: u64,
    seq: u64,
    timers: Vec<(u64, u64, Due)>,
    ping_every: u64,
    records: Vec<Json>,
}

fn arg_u(args: &Json, i: usize) -> Option<i64> {
    args.get(i).and_then(Json::as_f64).map(|f| f as i64)
}

fn arg_i(args: &Json, i: usize) -> Option<i32> {
    arg_u(args, i).map(|v| v as i32)
}

fn arg_b(args: &Json, i: usize) -> Option<bool> {
    args.get(i).and_then(Json::as_bool)
}

impl Replay {
    fn new(cfg: AaConfig) -> Self {
        set_wall(T0);
        let (tx, rx) = watch::channel(cfg);
        let (s, first) = Session::new(rx, "::FFFF:10.0.0.5%wlan0");
        let mut r = Self {
            s,
            _tx: tx,
            now: 0,
            seq: 0,
            timers: Vec::new(),
            ping_every: 0,
            records: Vec::new(),
        };
        r.apply(first);
        r
    }

    fn arm(&mut self, after: u64, due: Due) {
        self.seq += 1;
        self.timers.push((self.now + after, self.seq, due));
    }

    fn apply(&mut self, outs: Vec<Out>) {
        for o in outs {
            match o {
                Out::Send(f) => self.records.push(json!({ "send": frame_json(&f) })),
                Out::Control(v) => self.records.push(json!({ "control": v })),
                Out::End => self.records.push(json!({ "end": true })),
                Out::Destroy => self.records.push(json!({ "destroy": true })),
                Out::Event(e) => {
                    if let Some(e) = session_event_json(&e) {
                        self.records.push(json!({ "emit": e }));
                    }
                }
                Out::Ping(Some(every)) => {
                    self.ping_every = every.as_millis() as u64;
                    self.arm(self.ping_every, Due::Ping);
                }
                Out::Ping(None) => self.timers.retain(|t| t.2 != Due::Ping),
                Out::After(d, t) => self.arm(d.as_millis() as u64, Due::Timer(t)),
                Out::ShutdownDone => {}
            }
        }
    }

    async fn advance(&mut self, ms: u64) {
        let target = self.now + ms;
        loop {
            let next = self
                .timers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.0 <= target)
                .min_by_key(|(_, t)| (t.0, t.1))
                .map(|(i, _)| i);
            let Some(i) = next else { break };
            let (due, _, what) = self.timers[i];
            self.step_to(due).await;
            if what == Due::Ping {
                self.seq += 1;
                self.timers[i] = (due + self.ping_every, self.seq, what);
                let out = self.s.on_ping_tick();
                self.apply(out);
            } else {
                self.timers.remove(i);
                let Due::Timer(t) = what else { unreachable!() };
                let out = self.s.on_timer(t);
                self.apply(out);
            }
        }
        self.step_to(target).await;
    }

    async fn step_to(&mut self, at: u64) {
        tokio::time::advance(Duration::from_millis(at - self.now)).await;
        self.now = at;
        set_wall(T0 + at);
    }

    async fn op(&mut self, op: &Json) {
        self.records.push(json!({ "op": op }));
        if let Some(c) = op.get("control") {
            let out = self.s.on_link_control(c);
            self.apply(out);
        } else if let Some(m) = op.get("msg") {
            let out =
                self.s.on_message(u(&m[0]) as u8, u(&m[1]) as u16, &unhex(m[2].as_str().unwrap()));
            self.apply(out);
        } else if op.get("close").is_some() {
            let out = self.s.on_link_closed(None);
            self.apply(out);
        } else if let Some(e) = op.get("error") {
            let out = self.s.on_link_closed(Some(e.as_str().unwrap().to_string()));
            self.apply(out);
        } else if let Some(ms) = op.get("advance") {
            self.advance(ms.as_u64().unwrap()).await;
        } else if let Some(call) = op.get("call") {
            let out = self.call(call.as_str().unwrap(), &op["args"]);
            self.apply(out);
        }
    }

    fn sensor(&mut self, s: Sensor) -> Vec<Out> {
        self.s.send_sensor(&s)
    }

    fn call(&mut self, name: &str, a: &Json) -> Vec<Out> {
        match name {
            "sendTouch" => {
                let pointers: Vec<TouchPointer> = a[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| TouchPointer {
                        x: p["x"].as_i64().unwrap(),
                        y: p["y"].as_i64().unwrap(),
                        id: p["id"].as_i64().unwrap(),
                    })
                    .collect();
                self.s.send_touch(u(&a[0]), &pointers, arg_u(a, 2).unwrap_or(0) as u32)
            }
            "sendButton" => {
                let codes: Vec<u32> = match &a[0] {
                    Json::Array(c) => c.iter().map(u).collect(),
                    one => vec![u(one)],
                };
                self.s.send_button(&codes, a[1].as_bool().unwrap())
            }
            "sendRotary" => self.s.send_rotary(a[0].as_i64().unwrap()),
            "sendFuelData" => self.sensor(Sensor::Fuel {
                level_percent: arg_i(a, 0).unwrap(),
                range_km: arg_i(a, 1),
                low_fuel_warning: arg_b(a, 2),
            }),
            "sendSpeedData" => self.sensor(Sensor::Speed(arg_i(a, 0).unwrap())),
            "sendRpmData" => self.sensor(Sensor::Rpm(arg_i(a, 0).unwrap())),
            "sendGearData" => self.sensor(Sensor::Gear(arg_i(a, 0).unwrap())),
            "sendNightModeData" => self.sensor(Sensor::NightMode(arg_b(a, 0).unwrap())),
            "sendParkingBrakeData" => self.sensor(Sensor::ParkingBrake(arg_b(a, 0).unwrap())),
            "sendLightData" => self.sensor(Sensor::Light {
                head_light: arg_i(a, 0),
                hazard_lights: arg_b(a, 1),
                turn_indicator: arg_i(a, 2),
            }),
            "sendEnvironmentData" => self.sensor(Sensor::Environment {
                temperature_e3: arg_i(a, 0),
                pressure_e3: arg_i(a, 1),
            }),
            "sendOdometerData" => self.sensor(Sensor::Odometer {
                total_km_e1: arg_i(a, 0).unwrap(),
                trip_km_e1: arg_i(a, 1),
            }),
            "sendDrivingStatusData" => self.sensor(Sensor::DrivingStatus(arg_i(a, 0).unwrap())),
            "sendGpsLocationData" => {
                let o = &a[0];
                let f = |k: &str| o.get(k).and_then(Json::as_f64);
                self.sensor(Sensor::Gps(GpsFix {
                    lat_deg: f("latDeg").unwrap(),
                    lng_deg: f("lngDeg").unwrap(),
                    accuracy_m: f("accuracyM"),
                    altitude_m: f("altitudeM"),
                    speed_ms: f("speedMs"),
                    bearing_deg: f("bearingDeg"),
                }))
            }
            "sendVehicleEnergyModel" => {
                let o = &a[3];
                self.sensor(Sensor::VehicleEnergyModel {
                    capacity_wh: arg_u(a, 0).unwrap(),
                    current_wh: arg_u(a, 1).unwrap(),
                    range_m: arg_u(a, 2).unwrap(),
                    max_charge_power_w: o.get("maxChargePowerW").and_then(Json::as_i64),
                    max_discharge_power_w: o.get("maxDischargePowerW").and_then(Json::as_i64),
                    auxiliary_wh_per_km: o.get("auxiliaryWhPerKm").and_then(Json::as_f64),
                })
            }
            "requestVideoFocus" => self.s.request_video_focus(),
            "requestMainKeyframe" => self.s.request_main_keyframe(),
            "requestClusterKeyframe" => self.s.request_cluster_keyframe(),
            "forceClusterKeyframe" => self.s.force_cluster_keyframe(),
            "setClusterStreamActive" => self.s.set_cluster_stream_active(a[0].as_bool().unwrap()),
            "requestShutdown" => self.s.request_shutdown(arg_u(a, 0).unwrap_or(1) as u8),
            "close" => self.s.close(a.get(0).and_then(Json::as_str).unwrap_or("manual close")),
            "sendMediaSink" => self.s.send_media_sink(a[0].as_object().unwrap().clone()),
            other => panic!("no call {other}"),
        }
    }
}

/// The calls list has no recording.
fn session_event_json(e: &SessionEvent) -> Option<Json> {
    Some(match e {
        SessionEvent::Connected => json!(["connected"]),
        SessionEvent::Disconnected(r) => json!(["disconnected", r]),
        SessionEvent::Error(m) => json!(["error", m]),
        SessionEvent::VideoCodec(c) => json!(["video-codec", c.name()]),
        SessionEvent::ClusterVideoCodec(c) => json!(["cluster-video-codec", c.name()]),
        SessionEvent::AudioSetup { channel, sample_rate, channels } => {
            json!(["audio-setup", channel.name(), sample_rate, channels])
        }
        SessionEvent::Audio { channel, active, .. } => json!([
            if *active { "audio-start" } else { "audio-stop" },
            channel.name(),
            channel.channel()
        ]),
        SessionEvent::MicStart => json!(["mic-start", 9]),
        SessionEvent::MicStop => json!(["mic-stop", 9]),
        SessionEvent::VoiceSession(a) => json!(["voice-session", a]),
        SessionEvent::AudioFocus(t) => json!(["audio-focus", t]),
        SessionEvent::HostUiRequested => json!(["host-ui-requested"]),
        SessionEvent::DeviceInfo { name, model, instance_id, ip } => json!([
            "device-info",
            { "name": name, "model": model, "instanceId": instance_id, "ip": ip }
        ]),
        SessionEvent::Battery { ip, level, critical, time_remaining_s } => {
            let mut o = Map::new();
            o.insert("ip".into(), json!(ip));
            put(&mut o, "batteryLevel", *level);
            o.insert("batteryCritical".into(), json!(critical));
            put(&mut o, "batteryTimeRemaining", *time_remaining_s);
            json!(["device-status", o])
        }
        SessionEvent::Signal { ip, strength } => {
            json!(["device-status", { "ip": ip, "signalStrength": strength }])
        }
        SessionEvent::Calls { .. } => return None,
        SessionEvent::VideoFocusProjected => json!(["video-focus-projected"]),
        SessionEvent::ClusterVideoFocusProjected => json!(["cluster-video-focus-projected"]),
        SessionEvent::VideoStarted => json!(["video-started"]),
        SessionEvent::ClusterVideoStarted => json!(["cluster-video-started"]),
        SessionEvent::MediaMetadata(m) => json!(["media-metadata", metadata_json(m)]),
        SessionEvent::MediaStatus(s) => json!(["media-status", status_json(s)]),
        SessionEvent::Navigation(n) => nav_event_json(n),
    })
}

#[tokio::test(start_paused = true)]
async fn scripted_sessions_match() {
    for case in vectors()["sessions"].as_array().unwrap() {
        let mut r = Replay::new(ts_config(&case["config"]));
        for rec in case["records"].as_array().unwrap() {
            if let Some(op) = rec.get("op") {
                r.op(op).await;
            }
        }
        let expected = case["records"].as_array().unwrap();
        for (i, (got, want)) in r.records.iter().zip(expected).enumerate() {
            assert_eq!(got, want, "{} record {i}", case["name"]);
        }
        assert_eq!(r.records.len(), expected.len(), "{} records", case["name"]);
    }
}

fn bridge_event(name: &str, args: &[Json], formats: &mut [(u32, u32); 3]) -> SessionEvent {
    let s = |v: &Json, k: &str| v.get(k).and_then(Json::as_str).map(str::to_string);
    let side = |v: &Json| side_of(&v["turnSide"]);
    let channel = |v: &Json| AudioChannelType::from_name(v.as_str().unwrap()).unwrap();
    let slot = |c: AudioChannelType| c.channel() as usize - 4;
    match name {
        "connected" => SessionEvent::Connected,
        "disconnected" => SessionEvent::Disconnected(args[0].as_str().unwrap().into()),
        "device-info" => SessionEvent::DeviceInfo {
            name: s(&args[0], "name").unwrap(),
            model: s(&args[0], "model").unwrap(),
            instance_id: s(&args[0], "instanceId").unwrap(),
            ip: s(&args[0], "ip").unwrap(),
        },
        "device-status" => SessionEvent::Battery {
            ip: s(&args[0], "ip").unwrap(),
            level: opt_u(&args[0], "batteryLevel"),
            critical: args[0]["batteryCritical"].as_bool().unwrap(),
            time_remaining_s: opt_u(&args[0], "batteryTimeRemaining"),
        },
        "video-codec" | "cluster-video-codec" => {
            let codec = match args[0].as_str().unwrap() {
                "h265" => discovery::VideoCodec::H265,
                "vp9" => discovery::VideoCodec::Vp9,
                "av1" => discovery::VideoCodec::Av1,
                _ => discovery::VideoCodec::H264,
            };
            if name == "video-codec" {
                SessionEvent::VideoCodec(codec)
            } else {
                SessionEvent::ClusterVideoCodec(codec)
            }
        }
        "video-started" => SessionEvent::VideoStarted,
        "cluster-video-started" => SessionEvent::ClusterVideoStarted,
        "video-focus-projected" => SessionEvent::VideoFocusProjected,
        "cluster-video-focus-projected" => SessionEvent::ClusterVideoFocusProjected,
        "audio-setup" => {
            let c = channel(&args[0]);
            formats[slot(c)] = (u(&args[1]), u(&args[2]));
            SessionEvent::AudioSetup { channel: c, sample_rate: u(&args[1]), channels: u(&args[2]) }
        }
        "audio-start" | "audio-stop" => {
            let c = channel(&args[0]);
            let (sample_rate, channels) = formats[slot(c)];
            SessionEvent::Audio { channel: c, sample_rate, channels, active: name == "audio-start" }
        }
        "audio-focus" => SessionEvent::AudioFocus(u(&args[0]) as u8),
        "mic-start" => SessionEvent::MicStart,
        "mic-stop" => SessionEvent::MicStop,
        "voice-session" => SessionEvent::VoiceSession(args[0].as_bool().unwrap()),
        "host-ui-requested" => SessionEvent::HostUiRequested,
        "media-metadata" => {
            let m = &args[0];
            SessionEvent::MediaMetadata(MediaPlaybackMetadata {
                song: s(m, "song"),
                artist: s(m, "artist"),
                album: s(m, "album"),
                playlist: s(m, "playlist"),
                duration_seconds: opt_u(m, "durationSeconds"),
                rating: opt_u(m, "rating"),
                album_art: m.get("albumArt").and_then(Json::as_str).map(unhex),
            })
        }
        "media-status" => {
            let m = &args[0];
            let state = match m["state"].as_str() {
                Some("playing") => crate::channels::media_info::MediaPlaybackState::Playing,
                Some("paused") => crate::channels::media_info::MediaPlaybackState::Paused,
                Some("stopped") => crate::channels::media_info::MediaPlaybackState::Stopped,
                _ => crate::channels::media_info::MediaPlaybackState::Unknown,
            };
            SessionEvent::MediaStatus(MediaPlaybackStatus {
                state,
                media_source: s(m, "mediaSource"),
                playback_seconds: opt_u(m, "playbackSeconds"),
                ..Default::default()
            })
        }
        "nav-start" => SessionEvent::Navigation(NavigationEvent::Start),
        "nav-stop" => SessionEvent::Navigation(NavigationEvent::Stop),
        "nav-status" => {
            let state = match args[0]["state"].as_str() {
                Some("active") => crate::channels::navigation::NavigationState::Active,
                Some("inactive") => crate::channels::navigation::NavigationState::Inactive,
                Some("rerouting") => crate::channels::navigation::NavigationState::Rerouting,
                _ => crate::channels::navigation::NavigationState::Unavailable,
            };
            SessionEvent::Navigation(NavigationEvent::Status(state))
        }
        "nav-turn" => {
            let t = &args[0];
            SessionEvent::Navigation(NavigationEvent::Turn(
                crate::channels::navigation::NavigationTurnUpdate {
                    road: s(t, "road"),
                    turn_side: side(t),
                    event: t["event"].as_str().map(|e| {
                        NavigationTurnEvent::ALL.into_iter().find(|x| x.name() == e).unwrap()
                    }),
                    image: t.get("image").and_then(Json::as_str).map(unhex),
                    turn_number: opt_u(t, "turnNumber"),
                    turn_angle: opt_u(t, "turnAngle"),
                },
            ))
        }
        "nav-distance" => {
            let d = &args[0];
            SessionEvent::Navigation(NavigationEvent::Distance(
                crate::channels::navigation::NavigationDistanceUpdate {
                    distance_meters: u(&d["distanceMeters"]),
                    time_to_turn_seconds: u(&d["timeToTurnSeconds"]),
                    display_distance_e3: opt_u(d, "displayDistanceE3"),
                    display_unit: opt_u(d, "displayUnit"),
                },
            ))
        }
        "nav-state" => {
            let d = &args[0];
            SessionEvent::Navigation(NavigationEvent::State(
                crate::channels::navigation::NavigationStateUpdate {
                    maneuver_type: opt_u(d, "maneuverType"),
                    road_name: s(d, "roadName"),
                    cue: s(d, "cue"),
                    destination_address: s(d, "destinationAddress"),
                },
            ))
        }
        "nav-position" => {
            let d = &args[0];
            SessionEvent::Navigation(NavigationEvent::Position(
                crate::channels::navigation::NavigationPositionUpdate {
                    step_distance_meters: opt_u(d, "stepDistanceMeters"),
                    destination_meters: opt_u(d, "destinationMeters"),
                    time_to_arrival_seconds: opt_u(d, "timeToArrivalSeconds"),
                    eta_text: s(d, "etaText"),
                    ..Default::default()
                },
            ))
        }
        other => panic!("no bridge event {other}"),
    }
}

fn message(kind: &str, mut fields: Map<String, Json>) -> Json {
    fields.insert("kind".into(), json!(kind));
    json!({ "message": fields })
}

fn command(value: u32) -> Json {
    let mut o = Map::new();
    o.insert("value".into(), json!(value));
    message("Command", o)
}

fn navi_json(n: &Navigation) -> Json {
    let mut o = Map::new();
    put(&mut o, "NaviStatus", n.active.map(u8::from));
    put(&mut o, "NaviAPPName", n.app_name.clone());
    put(&mut o, "NaviRoadName", n.road_name.clone());
    put(&mut o, "NaviManeuverType", n.maneuver_type.map(|m| m as u8));
    put(&mut o, "NaviTurnSide", n.turn_side.map(|s| s as u8));
    put(&mut o, "NaviTurnAngle", n.turn_angle);
    put(&mut o, "NaviRoundaboutExitNumber", n.roundabout_exit_number);
    put(&mut o, "NaviRemainDistance", n.remain_distance);
    put(&mut o, "NaviDisplayDistanceE3", n.display_distance_e3);
    put(&mut o, "NaviDisplayDistanceUnit", n.display_distance_unit);
    put(&mut o, "NaviDestinationName", n.destination_name.clone());
    put(&mut o, "NaviDistanceToDestination", n.distance_to_destination);
    put(&mut o, "NaviTimeToDestination", n.time_to_destination);
    put(&mut o, "NaviETA", n.eta.clone());
    Json::Object(o)
}

fn now_playing_json(n: &NowPlaying) -> Vec<Json> {
    let mut out = Vec::new();
    let mut media = Map::new();
    put(&mut media, "MediaSongName", n.title.clone());
    put(&mut media, "MediaArtistName", n.artist.clone());
    put(&mut media, "MediaAlbumName", n.album.clone());
    put(&mut media, "MediaSongDuration", n.duration_ms);
    put(&mut media, "MediaPlayStatus", n.playing.map(u8::from));
    put(&mut media, "MediaAPPName", n.app.clone());
    put(&mut media, "MediaSongPlayTime", n.elapsed_ms);
    if !media.is_empty() {
        let mut o = Map::new();
        o.insert("mediaType".into(), json!(1));
        o.insert("payload".into(), json!({ "type": 1, "media": media }));
        out.push(message("MediaData", o));
    }
    if let Some(art) = &n.artwork {
        let mut o = Map::new();
        o.insert("mediaType".into(), json!(3));
        o.insert("payload".into(), json!({ "type": 3, "base64Image": base64(art) }));
        out.push(message("MediaData", o));
    }
    out
}

fn report_json(r: &Report) -> Vec<Json> {
    match r {
        Report::Connected => vec![json!({ "connected": true })],
        Report::Disconnected => vec![json!({ "disconnected": true })],
        Report::Device { name, model, instance_id, ip } => vec![json!({
            "presence": { "name": name, "model": model, "instanceId": instance_id, "ip": ip }
        })],
        Report::Status(s) => {
            let mut o = Map::new();
            o.insert("ip".into(), json!(s.ip));
            put(&mut o, "batteryLevel", s.battery_level);
            put(&mut o, "batteryCritical", s.battery_critical);
            put(&mut o, "batteryTimeRemaining", s.battery_time_remaining_s);
            put(&mut o, "signalStrength", s.signal_strength);
            vec![json!({ "status": o })]
        }
        Report::Calls(_) => vec![],
        Report::VideoCodec { cluster, codec } => vec![json!({
            "codec": [if *cluster { "cluster-video-codec" } else { "video-codec" }, codec.name()]
        })],
        Report::VideoFocus { cluster: false, projected: true } => vec![command(500)],
        Report::VideoFocus { cluster: false, projected: false } => vec![command(501)],
        Report::VideoFocus { cluster: true, .. } => vec![command(506)],
        Report::HostUiRequested => vec![command(3)],
        Report::Audio { stream, sample_rate, channels, active } => {
            let media = *stream == AudioChannelType::Media;
            let decode_type = match (sample_rate, channels) {
                (48000, 2) => 4,
                (16000, 1) => 5,
                other => panic!("no decode type for {other:?}"),
            };
            let cmd = match (media, active) {
                (true, true) => 10,
                (true, false) => 11,
                (false, true) => 6,
                (false, false) => 7,
            };
            let mut o = Map::new();
            o.insert("decodeType".into(), json!(decode_type));
            o.insert("audioType".into(), json!(AudioKind::of(*stream) as u8));
            o.insert("command".into(), json!(cmd));
            vec![message("AudioData", o)]
        }
        Report::Duck { level, duration_ms } => {
            let mut o = Map::new();
            let whole = level.fract() == 0.0;
            o.insert("level".into(), if whole { json!(*level as i64) } else { json!(level) });
            o.insert("durationMs".into(), json!(duration_ms));
            vec![message("DuckAudio", o)]
        }
        Report::NowPlaying(n) => now_playing_json(n),
        Report::Navigation(n) => {
            let mut o = Map::new();
            o.insert("metaType".into(), json!(200));
            o.insert("navi".into(), navi_json(n));
            o.insert("rawUtf8".into(), json!(""));
            vec![message("NavigationData", o)]
        }
        Report::NavigationImage(image) => {
            let mut o = Map::new();
            o.insert("metaType".into(), json!(201));
            o.insert("navi".into(), json!({ "NaviImageBase64": base64(image) }));
            o.insert("rawUtf8".into(), json!(""));
            vec![message("NavigationData", o)]
        }
    }
}

#[test]
fn the_event_bridge_matches() {
    let case = &vectors()["bridge"];
    let cfg = ts_config(&case["config"]);
    let outputs: Vec<(u32, Option<String>)> = case["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| (u(&o["streamId"]), o["tag"].as_str().map(str::to_string)))
        .collect();
    let feed = "/tmp/gst.feed";
    let audio_sink = |stream: u32, tag: &Option<String>| -> Option<Json> {
        let c = AudioChannelType::from_name(tag.as_deref()?)?;
        Some(json!({ "sink": { "feed": feed, "audio": [{ "ch": c.channel(), "id": stream }] } }))
    };
    let mut b = Bridge::default();
    let mut subscribed = true;
    let mut formats = [(48000, 2); 3];
    let mut got = Vec::new();
    for rec in case["records"].as_array().unwrap() {
        let Some(op) = rec.get("op") else { continue };
        got.push(json!({ "op": op }));
        let mut later = Vec::new();
        if let Some(ev) = op.get("ev") {
            let args = ev.as_array().unwrap();
            let name = args[0].as_str().unwrap();
            let event = bridge_event(name, &args[1..], &mut formats);
            let outs = b.on_event(event, &cfg);
            let mut now = Vec::new();
            for o in &outs {
                match o {
                    BridgeOut::Report(r) => now.extend(report_json(r)),
                    BridgeOut::StartMic(r) => now.push(json!({ "startMic": r })),
                    BridgeOut::StopMic(r) => now.push(json!({ "stopMic": r })),
                    BridgeOut::VideoSink { cluster, codec } => {
                        now.push(json!({ "primeVideo": cluster }));
                        let ch = if *cluster { 19 } else { 3 };
                        let id = if *cluster { 7 } else { 1 };
                        later.push(json!({
                            "sink": { "feed": feed, "video": [{ "ch": ch, "id": id, "codec": codec.name() }] }
                        }));
                    }
                    BridgeOut::AudioSinks => {
                        later.extend(outputs.iter().filter_map(|(s, t)| audio_sink(*s, t)));
                    }
                    BridgeOut::PrimeAudio { kind, sample_rate, channels, tag } => {
                        now.push(json!({ "primeAudio": [*kind as u8, sample_rate, channels, tag.name()] }));
                    }
                    BridgeOut::VideoStarted { cluster, width, height } => {
                        now.push(json!({ "videoStarted": [cluster, width, height] }));
                    }
                }
            }
            if name == "disconnected" {
                // The recording has the end first, this stack reports it last.
                if let Some(i) = now.iter().position(|r| r.get("disconnected").is_some()) {
                    let down = now.remove(i);
                    now.insert(0, down);
                }
                subscribed = false;
                now.push(json!({ "off": true }));
            }
            got.extend(now);
        } else if let Some(o) = op.get("output") {
            let tag = o[2].as_str().map(str::to_string);
            if subscribed {
                later.extend(audio_sink(u(&o[1]), &tag));
            }
        }
        got.extend(later);
    }
    let expected = case["records"].as_array().unwrap();
    for (i, (g, w)) in got.iter().zip(expected).enumerate() {
        assert_eq!(g, w, "bridge record {i}");
    }
    assert_eq!(got.len(), expected.len(), "bridge records");
}

fn livi_config(patch: &Json) -> livi_core_proto::config::Config {
    let mut v = serde_json::to_value(livi_core_proto::config::defaults()).unwrap();
    for (k, x) in patch.as_object().unwrap() {
        v[k] = x.clone();
    }
    serde_json::from_value(v).unwrap()
}

/// No microphone device key, the recording read it from the app's config when
/// the tap opened.
fn aa_config_json(c: &AaConfig) -> Json {
    let mut o = Map::new();
    let insets = |o: &mut Map<String, Json>, prefix: &str, i: &Insets| {
        o.insert(format!("{prefix}Top"), json!(i.top));
        o.insert(format!("{prefix}Bottom"), json!(i.bottom));
        o.insert(format!("{prefix}Left"), json!(i.left));
        o.insert(format!("{prefix}Right"), json!(i.right));
    };
    put(&mut o, "huName", c.hu_name.clone());
    put(&mut o, "videoWidth", c.video_width);
    put(&mut o, "videoHeight", c.video_height);
    put(&mut o, "videoDpi", c.video_dpi);
    put(&mut o, "videoFps", c.video_fps);
    put(&mut o, "pixelAspectRatioE4", c.pixel_aspect_ratio_e4);
    o.insert("displayWidth".into(), json!(c.display_width));
    o.insert("displayHeight".into(), json!(c.display_height));
    insets(&mut o, "mainViewArea", &c.main_view_area);
    insets(&mut o, "mainSafeArea", &c.main_safe_area);
    o.insert("driverPosition".into(), json!(c.driver_position));
    o.insert("wifiSsid".into(), json!(c.wifi_ssid));
    o.insert("wifiPassword".into(), json!(c.wifi_password));
    put(&mut o, "wifiChannel", c.wifi_channel);
    o.insert("fuelTypes".into(), json!(c.fuel_types));
    o.insert("evConnectorTypes".into(), json!(c.ev_connector_types));
    o.insert("hevcSupported".into(), json!(c.hevc_supported));
    o.insert("vp9Supported".into(), json!(c.vp9_supported));
    o.insert("av1Supported".into(), json!(c.av1_supported));
    put(&mut o, "initialNightMode", c.initial_night_mode);
    o.insert("clusterEnabled".into(), json!(c.cluster_enabled));
    o.insert("clusterWidth".into(), json!(c.cluster_width));
    o.insert("clusterHeight".into(), json!(c.cluster_height));
    put(&mut o, "clusterTierWidth", c.cluster_tier_width);
    put(&mut o, "clusterTierHeight", c.cluster_tier_height);
    put(&mut o, "clusterPixelAspectRatioE4", c.cluster_pixel_aspect_ratio_e4);
    o.insert("clusterFps".into(), json!(c.cluster_fps));
    put(&mut o, "clusterDpi", c.cluster_dpi);
    insets(&mut o, "clusterViewArea", &c.cluster_view_area);
    insets(&mut o, "clusterSafeArea", &c.cluster_safe_area);
    o.insert("disableAudioOutput".into(), json!(c.disable_audio_output));
    put(&mut o, "btMacAddress", c.bt_mac_address.clone());
    put(&mut o, "wifiBssid", c.wifi_bssid.clone());
    Json::Object(o)
}

fn command_of(name: &str) -> Command {
    use Command as C;
    match name {
        "requestHostUI" => C::RequestHostUi,
        "voiceAssistant" => C::VoiceAssistant,
        "voiceAssistantRelease" => C::VoiceAssistantRelease,
        "frame" => C::Frame,
        "left" => C::Left,
        "right" => C::Right,
        "up" => C::Up,
        "down" => C::Down,
        "selectDown" => C::SelectDown,
        "selectUp" => C::SelectUp,
        "back" => C::Back,
        "knobLeft" => C::KnobLeft,
        "knobRight" => C::KnobRight,
        "knobUp" => C::KnobUp,
        "knobDown" => C::KnobDown,
        "home" => C::Home,
        "play" => C::Play,
        "pause" => C::Pause,
        "playPause" => C::PlayPause,
        "next" => C::Next,
        "prev" => C::Prev,
        "acceptPhone" => C::AcceptPhone,
        "rejectPhone" => C::RejectPhone,
        "phoneKey0" => C::PhoneKey0,
        "phoneKey1" => C::PhoneKey1,
        "phoneKey2" => C::PhoneKey2,
        "phoneKey3" => C::PhoneKey3,
        "phoneKey4" => C::PhoneKey4,
        "phoneKey5" => C::PhoneKey5,
        "phoneKey6" => C::PhoneKey6,
        "phoneKey7" => C::PhoneKey7,
        "phoneKey8" => C::PhoneKey8,
        "phoneKey9" => C::PhoneKey9,
        "phoneKeyStar" => C::PhoneKeyStar,
        "phoneKeyHash" => C::PhoneKeyHash,
        "phoneKeyHookSwitch" => C::PhoneKeyHookSwitch,
        "voiceAssistantUiActive" => C::VoiceAssistantUiActive,
        "voiceAssistantUiIdle" => C::VoiceAssistantUiIdle,
        "requestVideoFocus" => C::RequestVideoFocus,
        "releaseVideoFocus" => C::ReleaseVideoFocus,
        "requestClusterFocus" => C::RequestClusterFocus,
        "requestClusterStreamFocus" => C::RequestClusterStreamFocus,
        other => panic!("no command {other}"),
    }
}

fn input_of(name: &str) -> InputCommand {
    use InputCommand as I;
    match name {
        "play" => I::Play,
        "pause" => I::Pause,
        "playPause" => I::PlayPause,
        "stop" => I::Stop,
        "next" => I::Next,
        "previous" => I::Previous,
        "fastForward" => I::FastForward,
        "rewind" => I::Rewind,
        "volumeUp" => I::VolumeUp,
        "volumeDown" => I::VolumeDown,
        "mute" => I::Mute,
        "acceptCall" => I::AcceptCall,
        "rejectCall" => I::RejectCall,
        "hookSwitch" => I::HookSwitch,
        "voiceAssistant" => I::VoiceAssistant,
        other => panic!("no input {other}"),
    }
}

struct Driver {
    helper: Link,
    from_driver: LinkReader,
    cmds: mpsc::UnboundedSender<SessionCmd>,
    media: Arc<FakeMedia>,
    marker: i32,
}

impl Driver {
    async fn start(cfg: AaConfig, cluster_active: bool) -> Self {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (link, reader) = link::attach(ours, "::FFFF:10.0.0.5%wlan0");
        let (helper, from_driver) = link::attach(theirs, "");
        let (cfg_tx, cfg_rx) = watch::channel(cfg);
        std::mem::forget(cfg_tx);
        let media = FakeMedia::new();
        let (cmds, cmd_rx) = mpsc::unbounded_channel();
        let (events, _) = mpsc::unbounded_channel();
        tokio::spawn(aa_session::run(
            1,
            link,
            reader,
            cfg_rx,
            media.clone(),
            cluster_active,
            cmd_rx,
            events,
        ));
        let mut d = Self { helper, from_driver, cmds, media, marker: 0 };
        d.helper
            .control(&json!({ "type": "ready", "peer": "p", "mic": "/tmp/aa-session-1.sock.mic" }));
        d.helper.send(&Frame::new(0, 0x0b, 0x0005, []));
        d.helper.send(&Frame::new(3, 0x0b, 0x8000, [0x08, 0x03]));
        d.through_link().await;
        d.media.take();
        d
    }

    async fn through_link(&mut self) -> Vec<Frame> {
        self.marker += 1;
        let ping = crate::wire::field_varint(1, i64::from(self.marker));
        self.helper.send(&Frame::new(0, 0x03, 0x000b, ping.clone()));
        self.until(Frame::new(0, 0x0b, 0x000c, ping)).await
    }

    async fn through_commands(&mut self) -> Vec<Frame> {
        self.marker += 1;
        self.cmds.send(SessionCmd::Sensor(Sensor::Gear(self.marker))).unwrap();
        let batch = crate::sensors::batch(&Sensor::Gear(self.marker)).unwrap();
        self.until(Frame::new(1, 0x0b, 0x8003, batch)).await
    }

    async fn until(&mut self, marker: Frame) -> Vec<Frame> {
        let mut frames = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), self.from_driver.next()).await {
                Ok(LinkItem::Message(f)) if f == marker => return frames,
                Ok(LinkItem::Message(f)) => {
                    let ping = f.ch == 0 && f.msg_id == 0x000b;
                    if !ping {
                        frames.push(f);
                    }
                }
                Ok(LinkItem::Control(_)) => {}
                other => panic!("the driver went quiet: {other:?}"),
            }
        }
    }
}

#[tokio::test]
async fn the_driver_matches() {
    for (n, case) in vectors()["aaSessions"].as_array().unwrap().iter().enumerate() {
        let cfg = livi_config(&case["patch"]);
        let seed = &case["seed"];
        let codecs = Codecs {
            hevc: seed["hevcSupported"].as_bool().unwrap(),
            vp9: seed["vp9Supported"].as_bool().unwrap(),
            av1: seed["av1Supported"].as_bool().unwrap(),
        };
        let wifi = if cfg.wifi_interface == "livi-link" {
            "DE:AD:BE:EF:00:01"
        } else {
            "11:22:33:44:55:66"
        };
        let addresses = Addresses {
            bt_mac: Some("AA:BB:CC:DD:EE:FF".into()),
            wifi_bssid: Some(wifi.into()),
            access_point: None,
        };
        let aa = from_livi(&cfg, codecs, seed["initialNightMode"].as_bool(), &addresses);
        assert_eq!(aa_config_json(&aa), case["aaCfg"], "driver {n} config");
        let g = Geometry::main(&aa);
        let touch =
            [g.tier_width, g.tier_height, g.inset.top, g.inset.bottom, g.inset.left, g.inset.right];
        assert_eq!(json!(touch), case["touch"], "driver {n} touch geometry");

        set_wall(T0);
        let mut d = Driver::start(aa, seed["clusterStreamActive"].as_bool().unwrap()).await;
        let records = case["records"].as_array().unwrap();
        let starts: Vec<usize> = records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.get("input").is_some())
            .map(|(i, _)| i)
            .collect();
        for (k, &at) in starts.iter().enumerate() {
            let input = &records[at]["input"];
            let end = starts.get(k + 1).copied().unwrap_or(records.len());
            let want = &records[at + 1..end];
            let frames = if let Some(t) = input.get("touch") {
                let action = match u(&t[2]) {
                    14 => TouchAction::Down,
                    15 => TouchAction::Move,
                    _ => TouchAction::Up,
                };
                let (x, y) = (t[0].as_f64().unwrap(), t[1].as_f64().unwrap());
                d.cmds.send(SessionCmd::Touch { action, x, y }).unwrap();
                d.through_commands().await
            } else if let Some(m) = input.get("multi") {
                let items = m
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| TouchItem {
                        x: p[0].as_f64().unwrap(),
                        y: p[1].as_f64().unwrap(),
                        action: match u(&p[2]) {
                            1 => TouchAction::Down,
                            2 => TouchAction::Move,
                            _ => TouchAction::Up,
                        },
                        id: u(&p[3]),
                    })
                    .collect();
                d.cmds.send(SessionCmd::MultiTouch(items)).unwrap();
                d.through_commands().await
            } else if let Some(c) = input.get("command") {
                d.cmds.send(SessionCmd::Command(command_of(c.as_str().unwrap()))).unwrap();
                d.through_commands().await
            } else if let Some(i) = input.get("input") {
                d.cmds.send(SessionCmd::Input(input_of(i.as_str().unwrap()))).unwrap();
                d.through_commands().await
            } else {
                let p = &input["phone"];
                d.helper.send(&Frame::new(
                    u(&p[0]) as u8,
                    0x0b,
                    u(&p[1]) as u16,
                    unhex(p[2].as_str().unwrap()),
                ));
                d.through_link().await
            };
            let sent: Vec<Json> = frames.iter().map(|f| json!({ "send": frame_json(f) })).collect();
            let want_sent: Vec<Json> =
                want.iter().filter(|r| r.get("send").is_some()).cloned().collect();
            assert_eq!(sent, want_sent, "driver {n} input {input}");
            let taps = d.media.take();
            let want_taps: Vec<Json> = want
                .iter()
                .filter(|r| r.get("micTap").is_some() || r.get("micTapClose").is_some())
                .cloned()
                .collect();
            assert_eq!(taps, want_taps, "driver {n} input {input} microphone");
        }
    }
}
