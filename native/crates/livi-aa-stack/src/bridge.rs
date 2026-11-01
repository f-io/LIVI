// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::channels::audio::AudioChannelType;
use crate::channels::media_info::{MediaPlaybackMetadata, MediaPlaybackState, MediaPlaybackStatus};
use crate::channels::nav_maneuver::{
    DrivingSide, ManeuverType, nav_maneuver_type_to_code, nav_maneuver_type_to_side,
    turn_event_to_maneuver_type, turn_side_to_navi_code,
};
use crate::channels::navigation::{NavigationEvent, NavigationState};
use crate::config::AaConfig;
use crate::discovery::VideoCodec;
use crate::log::detail;
use crate::media::AudioKind;
use crate::protocol::{PhoneCall, SessionEvent};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub app: Option<String>,
    pub duration_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub playing: Option<bool>,
    pub artwork: Option<Vec<u8>>,
}

impl NowPlaying {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Navigation {
    pub active: Option<bool>,
    pub app_name: Option<String>,
    pub road_name: Option<String>,
    pub maneuver_type: Option<ManeuverType>,
    pub turn_side: Option<DrivingSide>,
    pub turn_angle: Option<u32>,
    pub roundabout_exit_number: Option<u32>,
    pub remain_distance: Option<u32>,
    pub display_distance_e3: Option<u32>,
    pub display_distance_unit: Option<u32>,
    pub destination_name: Option<String>,
    pub distance_to_destination: Option<u32>,
    pub time_to_destination: Option<u32>,
    pub eta: Option<String>,
}

impl Navigation {
    fn merge(&mut self, patch: Navigation) {
        fn take<T>(into: &mut Option<T>, from: Option<T>) {
            if from.is_some() {
                *into = from;
            }
        }
        take(&mut self.active, patch.active);
        take(&mut self.app_name, patch.app_name);
        take(&mut self.road_name, patch.road_name);
        take(&mut self.maneuver_type, patch.maneuver_type);
        take(&mut self.turn_side, patch.turn_side);
        take(&mut self.turn_angle, patch.turn_angle);
        take(
            &mut self.roundabout_exit_number,
            patch.roundabout_exit_number,
        );
        take(&mut self.remain_distance, patch.remain_distance);
        take(&mut self.display_distance_e3, patch.display_distance_e3);
        take(&mut self.display_distance_unit, patch.display_distance_unit);
        take(&mut self.destination_name, patch.destination_name);
        take(
            &mut self.distance_to_destination,
            patch.distance_to_destination,
        );
        take(&mut self.time_to_destination, patch.time_to_destination);
        take(&mut self.eta, patch.eta);
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhoneStatus {
    pub ip: String,
    pub battery_level: Option<u32>,
    pub battery_critical: Option<bool>,
    pub battery_time_remaining_s: Option<u32>,
    pub signal_strength: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    Connected,
    Disconnected,
    Device {
        name: String,
        model: String,
        instance_id: String,
        ip: String,
    },
    Status(PhoneStatus),
    Calls(Vec<PhoneCall>),
    VideoCodec {
        cluster: bool,
        codec: VideoCodec,
    },
    VideoFocus {
        cluster: bool,
        projected: bool,
    },
    HostUiRequested,
    Audio {
        stream: AudioChannelType,
        sample_rate: u32,
        channels: u32,
        active: bool,
    },
    Duck {
        level: f64,
        duration_ms: u32,
    },
    NowPlaying(NowPlaying),
    Navigation(Navigation),
    NavigationImage(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeOut {
    Report(Report),
    StartMic(&'static str),
    StopMic(&'static str),
    VideoSink {
        cluster: bool,
        codec: VideoCodec,
    },
    AudioSinks,
    PrimeAudio {
        kind: AudioKind,
        sample_rate: u32,
        channels: u32,
        tag: AudioChannelType,
    },
    VideoStarted {
        cluster: bool,
        width: u32,
        height: u32,
    },
}

const GOOGLE_MAPS: &str = "Google Maps";

#[derive(Debug, Default)]
pub struct Bridge {
    navi: Navigation,
    navi_active: bool,
    navi_app: Option<String>,
    video_focus: bool,
    cluster_focus: bool,
}

fn report(out: &mut Vec<BridgeOut>, r: Report) {
    out.push(BridgeOut::Report(r));
}

impl Bridge {
    pub fn reset(&mut self) -> Vec<BridgeOut> {
        let mut out = Vec::new();
        self.navi = Navigation::default();
        self.navi_active = false;
        self.navi_app = None;
        if self.video_focus {
            self.video_focus = false;
            report(
                &mut out,
                Report::VideoFocus {
                    cluster: false,
                    projected: false,
                },
            );
        }
        out
    }

    pub fn on_event(&mut self, event: SessionEvent, cfg: &AaConfig) -> Vec<BridgeOut> {
        let mut out = Vec::new();
        match event {
            SessionEvent::Connected => {
                println!("[AaEventBridge] session connected");
                report(&mut out, Report::Connected);
                out.push(BridgeOut::AudioSinks);
            }
            SessionEvent::Disconnected(reason) => {
                println!("[AaEventBridge] session disconnected ({reason})");
                out.extend(self.reset());
                report(&mut out, Report::Disconnected);
            }
            SessionEvent::Error(e) => println!("[AaEventBridge] session error: {e}"),
            SessionEvent::DeviceInfo {
                name,
                model,
                instance_id,
                ip,
            } => {
                report(
                    &mut out,
                    Report::Device {
                        name,
                        model,
                        instance_id,
                        ip,
                    },
                );
            }
            SessionEvent::Battery {
                ip,
                level,
                critical,
                time_remaining_s,
            } => {
                report(
                    &mut out,
                    Report::Status(PhoneStatus {
                        ip,
                        battery_level: level,
                        battery_critical: Some(critical),
                        battery_time_remaining_s: time_remaining_s,
                        signal_strength: None,
                    }),
                );
            }
            SessionEvent::Signal { ip, strength } => {
                report(
                    &mut out,
                    Report::Status(PhoneStatus {
                        ip,
                        signal_strength: Some(strength),
                        ..Default::default()
                    }),
                );
            }
            SessionEvent::Calls { calls, .. } => report(&mut out, Report::Calls(calls)),
            SessionEvent::VideoFocusProjected => {
                self.video_focus = true;
                report(
                    &mut out,
                    Report::VideoFocus {
                        cluster: false,
                        projected: true,
                    },
                );
            }
            SessionEvent::ClusterVideoFocusProjected => {
                self.cluster_focus = true;
                report(
                    &mut out,
                    Report::VideoFocus {
                        cluster: true,
                        projected: true,
                    },
                );
            }
            SessionEvent::VideoCodec(codec) | SessionEvent::ClusterVideoCodec(codec) => {
                let cluster = matches!(event, SessionEvent::ClusterVideoCodec(_));
                detail!(
                    "[AaEventBridge] {}video codec {} (phone selection)",
                    if cluster { "cluster " } else { "" },
                    codec.name()
                );
                report(&mut out, Report::VideoCodec { cluster, codec });
                out.push(BridgeOut::VideoSink { cluster, codec });
            }
            SessionEvent::VideoStarted => {
                if !self.video_focus {
                    self.video_focus = true;
                    report(
                        &mut out,
                        Report::VideoFocus {
                            cluster: false,
                            projected: true,
                        },
                    );
                }
                out.push(BridgeOut::VideoStarted {
                    cluster: false,
                    width: cfg.video_width.unwrap_or(1280),
                    height: cfg.video_height.unwrap_or(720),
                });
            }
            SessionEvent::ClusterVideoStarted => {
                if !self.cluster_focus {
                    self.cluster_focus = true;
                    report(
                        &mut out,
                        Report::VideoFocus {
                            cluster: true,
                            projected: true,
                        },
                    );
                }
                out.push(BridgeOut::VideoStarted {
                    cluster: true,
                    width: cfg.cluster_tier_width.unwrap_or(0),
                    height: cfg.cluster_tier_height.unwrap_or(0),
                });
            }
            SessionEvent::AudioSetup {
                channel,
                sample_rate,
                channels,
            } => {
                out.push(BridgeOut::PrimeAudio {
                    kind: AudioKind::of(channel),
                    sample_rate,
                    channels,
                    tag: channel,
                });
            }
            SessionEvent::Audio {
                channel,
                sample_rate,
                channels,
                active,
            } => {
                detail!(
                    "[AaEventBridge] audio {} {} ({sample_rate}Hz {channels}ch)",
                    channel.name(),
                    if active { "start" } else { "stop" }
                );
                report(
                    &mut out,
                    Report::Audio {
                        stream: channel,
                        sample_rate,
                        channels,
                        active,
                    },
                );
            }
            SessionEvent::AudioFocus(focus_type) => {
                let (level, duration_ms) = if focus_type == 3 {
                    (0.2, 500)
                } else {
                    (1.0, 1500)
                };
                detail!("[AaEventBridge] audio-focus type={focus_type} -> duck level={level}");
                report(&mut out, Report::Duck { level, duration_ms });
            }
            SessionEvent::MicStart => out.push(BridgeOut::StartMic("mic-start")),
            SessionEvent::MicStop => out.push(BridgeOut::StopMic("mic-stop")),
            SessionEvent::VoiceSession(true) => {
                out.push(BridgeOut::StartMic("voice-session START"))
            }
            SessionEvent::VoiceSession(false) => out.push(BridgeOut::StopMic("voice-session END")),
            SessionEvent::HostUiRequested => {
                println!("[AaEventBridge] host UI requested");
                report(&mut out, Report::HostUiRequested);
            }
            SessionEvent::MediaMetadata(m) => self.metadata(&mut out, m),
            SessionEvent::MediaStatus(s) => self.status(&mut out, s),
            SessionEvent::Navigation(n) => self.navigation(&mut out, n),
        }
        out
    }

    fn metadata(&mut self, out: &mut Vec<BridgeOut>, m: MediaPlaybackMetadata) {
        let now = NowPlaying {
            title: m.song,
            artist: m.artist,
            album: m.album,
            duration_ms: m.duration_seconds.map(|d| u64::from(d) * 1000),
            artwork: m.album_art.filter(|a| !a.is_empty()),
            ..Default::default()
        };
        if !now.is_empty() {
            report(out, Report::NowPlaying(now));
        }
    }

    fn status(&mut self, out: &mut Vec<BridgeOut>, s: MediaPlaybackStatus) {
        report(
            out,
            Report::NowPlaying(NowPlaying {
                playing: Some(s.state == MediaPlaybackState::Playing),
                app: s.media_source,
                elapsed_ms: s.playback_seconds.map(|p| u64::from(p) * 1000),
                ..Default::default()
            }),
        );
    }

    fn navigation(&mut self, out: &mut Vec<BridgeOut>, event: NavigationEvent) {
        match event {
            NavigationEvent::Start => {
                self.navi_app = Some(GOOGLE_MAPS.to_string());
                self.navi_active = true;
                self.publish(
                    out,
                    Navigation {
                        active: Some(true),
                        app_name: self.navi_app.clone(),
                        ..Default::default()
                    },
                );
            }
            NavigationEvent::Stop => {
                self.navi_active = false;
                self.publish(
                    out,
                    Navigation {
                        active: Some(false),
                        ..Default::default()
                    },
                );
            }
            NavigationEvent::Status(state) => {
                self.navi_active =
                    matches!(state, NavigationState::Active | NavigationState::Rerouting);
                let active = Some(self.navi_active);
                self.publish(
                    out,
                    Navigation {
                        active,
                        ..Default::default()
                    },
                );
            }
            NavigationEvent::Turn(t) => {
                let patch = Navigation {
                    road_name: t.road.clone(),
                    maneuver_type: turn_event_to_maneuver_type(t.event, t.turn_side),
                    turn_side: turn_side_to_navi_code(t.turn_side),
                    turn_angle: t.turn_angle,
                    roundabout_exit_number: t.turn_number,
                    ..Default::default()
                };
                if !patch.is_empty() {
                    self.publish(out, patch);
                }
                if let Some(image) = t.image.filter(|i| !i.is_empty()) {
                    report(out, Report::NavigationImage(image));
                }
            }
            NavigationEvent::Distance(d) => {
                self.publish(
                    out,
                    Navigation {
                        remain_distance: Some(d.distance_meters),
                        display_distance_e3: d.display_distance_e3,
                        display_distance_unit: d.display_unit,
                        ..Default::default()
                    },
                );
            }
            NavigationEvent::State(s) => {
                let patch = Navigation {
                    maneuver_type: nav_maneuver_type_to_code(s.maneuver_type),
                    turn_side: nav_maneuver_type_to_side(s.maneuver_type),
                    road_name: s.road_name.filter(|r| !r.is_empty()),
                    destination_name: s.destination_address.filter(|d| !d.is_empty()),
                    ..Default::default()
                };
                if !patch.is_empty() {
                    self.publish(out, patch);
                }
            }
            NavigationEvent::Position(p) => {
                let patch = Navigation {
                    remain_distance: p.step_distance_meters,
                    distance_to_destination: p.destination_meters,
                    time_to_destination: p.time_to_arrival_seconds,
                    eta: p.eta_text.filter(|e| !e.is_empty()),
                    ..Default::default()
                };
                if !patch.is_empty() {
                    self.publish(out, patch);
                }
            }
        }
    }

    fn publish(&mut self, out: &mut Vec<BridgeOut>, patch: Navigation) {
        self.navi.merge(patch);
        if self.navi_app.is_some() && self.navi.app_name.is_none() {
            self.navi.app_name = self.navi_app.clone();
        }
        report(out, Report::Navigation(self.navi.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::navigation::{
        NavigationDistanceUpdate, NavigationPositionUpdate, NavigationStateUpdate,
        NavigationTurnEvent, NavigationTurnSide, NavigationTurnUpdate,
    };

    fn reports(out: Vec<BridgeOut>) -> Vec<Report> {
        out.into_iter()
            .filter_map(|o| match o {
                BridgeOut::Report(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn guidance_accumulates_and_starts_over() {
        let mut b = Bridge::default();
        let cfg = AaConfig::default();
        let nav = |b: &mut Bridge, e| reports(b.on_event(SessionEvent::Navigation(e), &cfg));
        let start = nav(&mut b, NavigationEvent::Start);
        assert_eq!(
            start,
            [Report::Navigation(Navigation {
                active: Some(true),
                app_name: Some("Google Maps".into()),
                ..Default::default()
            })]
        );
        let turn = NavigationTurnUpdate {
            road: Some("A9".into()),
            event: Some(NavigationTurnEvent::Turn),
            turn_side: Some(NavigationTurnSide::Right),
            image: Some(vec![1]),
            ..Default::default()
        };
        let out = nav(&mut b, NavigationEvent::Turn(turn));
        let Report::Navigation(n) = &out[0] else {
            panic!()
        };
        assert_eq!(n.maneuver_type, Some(ManeuverType::RightTurn));
        assert_eq!(n.turn_side, Some(DrivingSide::Right));
        assert_eq!(out[1], Report::NavigationImage(vec![1]));
        assert!(
            nav(
                &mut b,
                NavigationEvent::Turn(NavigationTurnUpdate::default())
            )
            .is_empty()
        );
        let out = nav(
            &mut b,
            NavigationEvent::Distance(NavigationDistanceUpdate {
                distance_meters: 80,
                ..Default::default()
            }),
        );
        let Report::Navigation(n) = &out[0] else {
            panic!()
        };
        assert_eq!(
            (n.remain_distance, n.road_name.as_deref()),
            (Some(80), Some("A9"))
        );
        let out = nav(
            &mut b,
            NavigationEvent::State(NavigationStateUpdate {
                maneuver_type: Some(41),
                road_name: Some(String::new()),
                ..Default::default()
            }),
        );
        let Report::Navigation(n) = &out[0] else {
            panic!()
        };
        assert_eq!(n.maneuver_type, Some(ManeuverType::ArrivedLeft));
        assert_eq!(n.road_name.as_deref(), Some("A9"));
        assert!(
            nav(
                &mut b,
                NavigationEvent::State(NavigationStateUpdate::default())
            )
            .is_empty()
        );
        assert!(
            nav(
                &mut b,
                NavigationEvent::Position(NavigationPositionUpdate::default())
            )
            .is_empty()
        );
        let out = nav(&mut b, NavigationEvent::Status(NavigationState::Rerouting));
        let Report::Navigation(n) = &out[0] else {
            panic!()
        };
        assert_eq!(n.active, Some(true));
        let out = nav(&mut b, NavigationEvent::Stop);
        let Report::Navigation(n) = &out[0] else {
            panic!()
        };
        assert_eq!(
            (n.active, n.app_name.as_deref()),
            (Some(false), Some("Google Maps"))
        );

        b.on_event(SessionEvent::VideoFocusProjected, &cfg);
        let out = reports(b.on_event(SessionEvent::Disconnected("x".into()), &cfg));
        assert_eq!(
            out,
            [
                Report::VideoFocus {
                    cluster: false,
                    projected: false
                },
                Report::Disconnected
            ]
        );
        let out = nav(&mut b, NavigationEvent::Stop);
        assert_eq!(
            out,
            [Report::Navigation(Navigation {
                active: Some(false),
                ..Default::default()
            })]
        );
    }

    #[test]
    fn focus_media_and_audio() {
        let mut b = Bridge::default();
        let cfg = AaConfig {
            cluster_tier_width: Some(800),
            cluster_tier_height: Some(480),
            ..Default::default()
        };
        let out = b.on_event(SessionEvent::VideoStarted, &cfg);
        assert_eq!(
            out,
            [
                BridgeOut::Report(Report::VideoFocus {
                    cluster: false,
                    projected: true
                }),
                BridgeOut::VideoStarted {
                    cluster: false,
                    width: 1280,
                    height: 720
                },
            ]
        );
        assert_eq!(b.on_event(SessionEvent::VideoStarted, &cfg).len(), 1);
        let out = b.on_event(SessionEvent::ClusterVideoStarted, &cfg);
        assert_eq!(
            out[1],
            BridgeOut::VideoStarted {
                cluster: true,
                width: 800,
                height: 480
            }
        );
        assert_eq!(
            reports(b.on_event(SessionEvent::AudioFocus(3), &cfg)),
            [Report::Duck {
                level: 0.2,
                duration_ms: 500
            }]
        );
        assert_eq!(
            reports(b.on_event(SessionEvent::AudioFocus(1), &cfg)),
            [Report::Duck {
                level: 1.0,
                duration_ms: 1500
            }]
        );
        let meta = MediaPlaybackMetadata {
            album_art: Some(Vec::new()),
            ..Default::default()
        };
        assert!(
            b.on_event(SessionEvent::MediaMetadata(meta), &cfg)
                .is_empty()
        );
        let meta = MediaPlaybackMetadata {
            song: Some("S".into()),
            duration_seconds: Some(3),
            album_art: Some(vec![9]),
            ..Default::default()
        };
        assert_eq!(
            reports(b.on_event(SessionEvent::MediaMetadata(meta), &cfg)),
            [Report::NowPlaying(NowPlaying {
                title: Some("S".into()),
                duration_ms: Some(3000),
                artwork: Some(vec![9]),
                ..Default::default()
            })]
        );
        let status = MediaPlaybackStatus {
            playback_seconds: Some(2),
            ..Default::default()
        };
        assert_eq!(
            reports(b.on_event(SessionEvent::MediaStatus(status), &cfg)),
            [Report::NowPlaying(NowPlaying {
                playing: Some(false),
                elapsed_ms: Some(2000),
                ..Default::default()
            })]
        );
        assert_eq!(
            b.on_event(
                SessionEvent::AudioSetup {
                    channel: AudioChannelType::System,
                    sample_rate: 16000,
                    channels: 1
                },
                &cfg
            ),
            [BridgeOut::PrimeAudio {
                kind: AudioKind::Alert,
                sample_rate: 16000,
                channels: 1,
                tag: AudioChannelType::System
            }]
        );
        assert_eq!(
            b.on_event(SessionEvent::VoiceSession(true), &cfg),
            [BridgeOut::StartMic("voice-session START")]
        );
        assert_eq!(
            b.on_event(SessionEvent::MicStop, &cfg),
            [BridgeOut::StopMic("mic-stop")]
        );
        assert_eq!(
            b.on_event(SessionEvent::Connected, &cfg),
            [BridgeOut::Report(Report::Connected), BridgeOut::AudioSinks]
        );
    }
}
