// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::config::{AppearanceMode, defaults};
use livi_core_proto::input::Point;
use livi_core_proto::state::Maneuver;
use livi_cp::airplay::{AirplayCommand, AirplayRemote, AirplaySender};
use livi_cp::stack::AudioProfile;
use livi_media::compositor::CompositorControl;
use livi_media::output::Output;
use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;

use super::*;
use crate::files::config_file::tests::TempDir;

struct Rig {
    dir: TempDir,
    hub: Arc<Hub>,
    p: Projection,
    cmds: mpsc::UnboundedReceiver<CpCmd>,
    aa: mpsc::UnboundedReceiver<AaCmd>,
    crop: watch::Sender<Option<Crop>>,
    io: mpsc::UnboundedReceiver<Io>,
    telemetry: mpsc::UnboundedReceiver<Value>,
    patches: mpsc::UnboundedReceiver<Value>,
}

fn livi() -> PerScreen<Front> {
    PerScreen {
        main: Front::Livi,
        cluster: Front::Livi,
        aux: Front::Livi,
    }
}

#[test]
fn the_projection_start_page_shows_a_phone_before_the_ui_reports() {
    let mut cfg = defaults();
    assert_eq!(
        first_wants(&cfg.start_page),
        PerScreen {
            main: Front::Projection,
            ..livi()
        }
    );
    cfg.start_page = "/media".into();
    assert_eq!(
        first_wants(&cfg.start_page),
        livi(),
        "another start page waits for the UI"
    );
}

#[test]
fn the_last_start_page_comes_from_the_saved_status() {
    let mut cfg = defaults();
    cfg.start_page = "last".into();
    assert_eq!(start_page(&cfg, Some("/media/dabplus")), "/media/dabplus");
    assert_eq!(start_page(&cfg, None), "/");
    cfg.start_page = "/settings".into();
    assert_eq!(start_page(&cfg, Some("/media")), "/settings");
}

impl Rig {
    fn new() -> Self {
        let dir = TempDir::new();
        let hub = Arc::new(Hub::new(State {
            front: livi(),
            sessions: Default::default(),
            now_playing: Default::default(),
            video_playback: None,
            telemetry: Default::default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            cluster: Default::default(),
            dab: Default::default(),
            fm: Default::default(),
            cameras: Vec::new(),
            mfi: Default::default(),
            config: defaults(),
        }));
        let (tx, cmds) = mpsc::unbounded_channel();
        let (aa_tx, mut aa) = mpsc::unbounded_channel();
        let (crop, aa_crop) = watch::channel(None);
        let gst = Output::new(dir.0.join("gst.sock"));
        let ctrl = CompositorControl::connect(dir.0.join("ctrl"));
        let plane = MainPlane::new(gst.clone(), ctrl.clone());
        let clusters = ClusterPlanes::new(gst, ctrl);
        let devices = Devices::new(
            &dir.0.join("devices.json"),
            livi_cp::helper_sock::HelperSock::new(dir.0.join("cp-bt.sock")),
            crate::devices::Shared::new(dir.0.join("livi-shared.sock")),
        );
        let (phone_io, io) = mpsc::unbounded_channel();
        let (telemetry_tx, telemetry) = mpsc::unbounded_channel();
        let (settings, patches) = mpsc::unbounded_channel();
        let drivers = Drivers {
            cp: CpHandle::from_sender(tx),
            aa: AaHandle::from_sender(aa_tx),
            aa_crop,
            phone_io,
            phone_back: mpsc::unbounded_channel().1,
            telemetry: telemetry_tx,
            settings,
            themes: dir.0.join("themes"),
        };
        let p = Projection::new(
            hub.clone(),
            drivers,
            plane,
            clusters,
            Arc::new(Notify::new()),
            devices,
            livi(),
        );
        assert_eq!(aa.try_recv(), Ok(AaCmd::ClusterActive(false)));
        let mut rig = Self {
            dir,
            hub,
            p,
            cmds,
            aa,
            crop,
            io,
            telemetry,
            patches,
        };
        assert_eq!(
            rig.drain(),
            [CpCmd::NightModes([true; 4]), CpCmd::ClusterActive(false)]
        );
        rig
    }

    fn io_drain(&mut self) -> Vec<Io> {
        let mut out = Vec::new();
        while let Ok(io) = self.io.try_recv() {
            out.push(io);
        }
        out
    }

    fn aa_drain(&mut self) -> Vec<AaCmd> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.aa.try_recv() {
            out.push(cmd);
        }
        out
    }

    fn drain(&mut self) -> Vec<CpCmd> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.cmds.try_recv() {
            out.push(cmd);
        }
        out
    }

    fn connect(&mut self, id: SessionId, wired: bool) {
        self.p.on_event(CpEvent::Connected {
            id,
            wired,
            controller_id: None,
        });
        self.p.main_video_shown();
    }

    fn device(&mut self, id: SessionId, wired: bool, bt: &str, wifi: &str) {
        self.p.on_event(CpEvent::Presence {
            id,
            wired,
            presence: Presence::Device {
                bt_mac: bt.into(),
                wifi_mac: wifi.into(),
                ip: String::new(),
                usb_udid: String::new(),
                name: String::new(),
                model: String::new(),
            },
        });
    }

    fn want(&mut self, front: Front) {
        self.p.asked(PerScreen {
            main: front,
            ..livi()
        });
    }

    fn front(&self) -> Front {
        self.hub.watch().borrow().front.main
    }

    fn now_playing(&self) -> NowPlaying {
        self.hub.watch().borrow().now_playing.clone()
    }

    fn navigation(&self) -> Navigation {
        self.hub.watch().borrow().navigation.clone()
    }

    fn meta(&mut self, id: SessionId, event: Value) {
        let event = serde_json::from_value(event).expect("a helper event");
        self.p.on_event(CpEvent::Metadata { id, event });
    }

    fn sessions(&self) -> (Option<Protocol>, u32, u32) {
        let s = self.hub.watch().borrow().sessions;
        (s.active, s.position, s.total)
    }

    fn settings(&mut self, change: impl FnOnce(&mut Config)) {
        self.hub.update(|s| change(&mut s.config));
        let (cfg, telemetry) = {
            let s = self.hub.watch();
            let s = s.borrow();
            (s.config.clone(), s.telemetry.clone())
        };
        self.p.on_state(cfg, telemetry);
    }
}

fn stack(id: SessionId, cmd: StackCmd) -> CpCmd {
    CpCmd::Stack { id, cmd }
}

fn volume(id: SessionId, kind: AudioKind, level: f64, ramp_ms: u32) -> CpCmd {
    stack(
        id,
        StackCmd::StreamVolume {
            kind,
            level,
            ramp_ms,
        },
    )
}

fn stream_volumes(music: f64, nav: f64, call: f64, voice: f64) -> CpCmd {
    let levels = json!({
        "audioVolume": music,
        "navVolume": nav,
        "callVolume": call,
        "voiceAssistantVolume": voice,
    });
    CpCmd::Gauges(levels.as_object().cloned().unwrap())
}

fn home() -> StackCmd {
    knob(KnobState {
        home: true,
        ..Default::default()
    })
}

fn press(code: &str) -> Input {
    Input::Key {
        code: code.into(),
        down: true,
    }
}

#[tokio::test]
async fn a_session_without_video_keeps_the_ui_in_front() {
    let mut rig = Rig::new();
    rig.want(Front::Projection);
    rig.p.on_event(CpEvent::Connected {
        id: 1,
        wired: true,
        controller_id: None,
    });
    assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 1));
    assert_eq!(rig.front(), Front::Livi);
    rig.p.on_event(CpEvent::VideoCodec {
        id: 1,
        cluster: false,
        codec: livi_cp::stack::ScreenCodec::H264,
    });
    assert_eq!(rig.front(), Front::Livi, "the codec alone shows nothing");
    rig.p.main_video_shown();
    assert_eq!(rig.front(), Front::Projection);
}

#[tokio::test]
async fn the_first_phone_projects_where_the_ui_asks() {
    let mut rig = Rig::new();
    rig.want(Front::Projection);
    assert_eq!(rig.front(), Front::Livi);
    rig.connect(1, true);
    assert_eq!(rig.front(), Front::Projection);
    assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 1));
    assert_eq!(
        rig.drain(),
        [
            volume(1, AudioKind::Media, 0.95, RAMP_UP_MS),
            stack(1, StackCmd::Keyframe),
            stack(1, StackCmd::VideoActive(true)),
            stack(1, StackCmd::AudioActive(true)),
            volume(1, AudioKind::Media, 0.95, 0),
            volume(1, AudioKind::Alert, 0.95, 0),
            volume(1, AudioKind::Speech, 0.95, 0),
            volume(1, AudioKind::Call, 0.95, 0),
        ]
    );
    rig.want(Front::Camera);
    assert_eq!(rig.front(), Front::Camera);
    rig.want(Front::Projection);
    rig.want(Front::Projection);
    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    assert_eq!(rig.front(), Front::Livi);
    assert_eq!(rig.sessions(), (None, 0, 0));
    assert_eq!(
        rig.drain(),
        [
            stack(1, home()),
            stack(1, StackCmd::Keyframe),
            stack(1, StackCmd::VideoActive(false)),
            stack(1, StackCmd::AudioActive(false)),
        ]
    );
    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    assert!(rig.p.sessions.is_empty());
}

#[tokio::test]
async fn a_held_phone_moves_up() {
    let mut rig = Rig::new();
    rig.connect(1, false);
    rig.device(2, false, "AA:BB", "cc:dd");
    rig.drain();
    assert!(rig.p.sessions[0].active && !rig.p.sessions[1].active);
    assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 2));
    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    assert!(rig.p.sessions[0].active);
    assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 1));
    let cmds = rig.drain();
    assert_eq!(cmds[1], stack(2, StackCmd::Keyframe));
    assert!(cmds.contains(&stack(2, StackCmd::VideoActive(true))));
    rig.p.on_event(CpEvent::Presence {
        id: 2,
        wired: false,
        presence: Presence::Active { ip: "x".into() },
    });
    rig.p.on_event(CpEvent::Presence {
        id: 2,
        wired: false,
        presence: Presence::Status {
            battery_level: None,
            battery_charging: None,
            signal_strength: None,
            carrier_name: None,
        },
    });
    assert!(rig.drain().is_empty());
}

#[tokio::test]
async fn the_connection_takes_over_its_placeholder_and_the_cable_takes_over_wireless() {
    let mut rig = Rig::new();
    rig.device(5, false, "AA:BB", "");
    rig.drain();
    rig.device(6, false, "aa:bb", "11:22");
    assert_eq!(rig.p.sessions.len(), 1);
    assert_eq!(rig.p.sessions[0].link, Link::Cp(6));
    assert!(rig.drain().contains(&CpCmd::Close { id: 5 }));

    rig.device(7, true, "AA:BB", "33:44");
    assert_eq!(rig.p.sessions.len(), 1);
    assert_eq!(rig.p.sessions[0].link, Link::Cp(7));
    assert_eq!(rig.p.sessions[0].transport, Transport::Usb);
    assert_eq!(rig.p.sessions[0].device.wifi_mac.as_deref(), Some("11:22"));
    assert!(rig.drain().contains(&CpCmd::Close { id: 6 }));

    rig.device(8, false, "AA:BB", "");
    assert_eq!(rig.p.sessions.len(), 1);
    assert_eq!(rig.p.sessions[0].link, Link::Cp(8));
    assert!(rig.drain().contains(&CpCmd::Close { id: 7 }));
    let pos = rig.p.by_identity(&Ids {
        usb_udid: Some("u".into()),
        ..Default::default()
    });
    assert_eq!(pos, None);
}

#[tokio::test]
async fn levels_follow_the_config_and_the_ducking() {
    let mut rig = Rig::new();
    rig.connect(1, true);
    rig.drain();
    rig.p.on_event(CpEvent::Duck {
        id: 1,
        level: 0.5,
        duration_ms: 300,
    });
    assert_eq!(rig.drain(), [volume(1, AudioKind::Media, 0.475, 300)]);
    rig.p.on_event(CpEvent::Duck {
        id: 1,
        level: 1.0,
        duration_ms: 100,
    });
    assert_eq!(rig.drain(), [volume(1, AudioKind::Media, 0.95, 100)]);
    rig.p.on_event(CpEvent::Duck {
        id: 9,
        level: 0.1,
        duration_ms: 0,
    });
    assert!(rig.drain().is_empty());

    let mut cfg = defaults();
    cfg.nav_volume = 0.5;
    cfg.call_volume = 1.0;
    cfg.appearance_mode = AppearanceMode::Night;
    rig.p.on_config(&cfg);
    assert_eq!(
        rig.drain(),
        [
            volume(1, AudioKind::Alert, 0.5, RAMP_DOWN_MS),
            volume(1, AudioKind::Call, 1.0, RAMP_UP_MS),
            CpCmd::NightModes([true; 4]),
            stream_volumes(cfg.audio_volume, 0.5, 1.0, cfg.voice_assistant_volume),
        ]
    );
    let media = AudioProfile {
        kind: AudioKind::Call,
        label: "telephony".into(),
    };
    rig.p.on_event(CpEvent::AudioActive {
        id: 1,
        profile: media.clone(),
        active: true,
    });
    assert_eq!(rig.drain(), [volume(1, AudioKind::Call, 1.0, 0)]);
    rig.p.on_event(CpEvent::AudioActive {
        id: 1,
        profile: media,
        active: false,
    });
    assert!(rig.drain().is_empty());
    assert_eq!(AppearanceMode::Day.night_mode(), Some(false));
}

#[tokio::test]
async fn buttons_and_touch_go_to_the_active_phone() {
    let mut rig = Rig::new();
    rig.p.media(MediaControl::Next);
    assert!(rig.drain().is_empty());
    rig.connect(1, true);
    rig.drain();
    for (control, index) in [
        (MediaControl::Play, media_button::PLAY),
        (MediaControl::Pause, media_button::PAUSE),
        (MediaControl::PlayPause, media_button::PLAY_PAUSE),
        (MediaControl::Next, media_button::NEXT),
        (MediaControl::Prev, media_button::PREV),
    ] {
        rig.p.media(control);
        assert_eq!(rig.drain(), [stack(1, StackCmd::Media(index))]);
    }
    let touch = Input::Pointer {
        screen: Screen::Main,
        points: vec![
            Point {
                id: 0,
                x: 0.25,
                y: 1.5,
                phase: Phase::Down,
            },
            Point {
                id: 1,
                x: 0.5,
                y: 0.5,
                phase: Phase::Up,
            },
        ],
    };
    rig.p.input(touch.clone());
    assert!(rig.drain().is_empty());
    rig.want(Front::Projection);
    rig.p.input(touch);
    assert_eq!(
        rig.drain(),
        [
            stack(1, home()),
            stack(1, StackCmd::Keyframe),
            stack(
                1,
                StackCmd::Touches(vec![
                    Touch {
                        x: 0.25,
                        y: 1.0,
                        down: true
                    },
                    Touch {
                        x: 0.5,
                        y: 0.5,
                        down: false
                    },
                ])
            )
        ]
    );
    rig.p.input(Input::Pointer {
        screen: Screen::Main,
        points: vec![],
    });
    let dash = Point {
        id: 0,
        x: 0.5,
        y: 0.5,
        phase: Phase::Down,
    };
    rig.p.input(Input::Pointer {
        screen: Screen::Cluster,
        points: vec![dash],
    });
    rig.p.keyframe();
    assert_eq!(rig.drain(), [stack(1, StackCmd::Keyframe)]);
}

#[tokio::test]
async fn the_player_started_last_stops_the_other() {
    let mut rig = Rig::new();
    let (tx, mut radio) = mpsc::unbounded_channel();
    rig.p.radio = Some(tx);
    rig.connect(1, true);
    rig.drain();
    while radio.try_recv().is_ok() {}

    rig.p.follow_players(true, false);
    rig.p.follow_players(true, true);
    assert_eq!(
        rig.drain(),
        [stack(1, StackCmd::Media(media_button::PAUSE))]
    );
    assert!(radio.try_recv().is_err());

    rig.p.follow_players(false, true);
    rig.p.follow_players(true, true);
    assert_eq!(radio.try_recv(), Ok(crate::radio::RadioAsk::Stop));
    assert!(rig.drain().is_empty());
}

#[tokio::test]
async fn the_phone_ducks_the_radio_too() {
    let mut rig = Rig::new();
    let (tx, mut radio) = mpsc::unbounded_channel();
    rig.p.radio = Some(tx);
    rig.connect(1, true);
    while radio.try_recv().is_ok() {}
    rig.p.on_event(CpEvent::Duck {
        id: 1,
        level: 0.3,
        duration_ms: 200,
    });
    assert_eq!(
        radio.try_recv(),
        Ok(crate::radio::RadioAsk::Duck {
            level: 0.3,
            ramp_ms: 200
        })
    );
    rig.p.on_event(CpEvent::Duck {
        id: 9,
        level: 0.1,
        duration_ms: 0,
    });
    assert!(radio.try_recv().is_err(), "only the phone in front ducks");
}

#[tokio::test]
async fn carplay_keeps_the_media_keys_while_the_radio_plays() {
    let mut rig = Rig::new();
    let (tx, mut radio) = mpsc::unbounded_channel();
    rig.p.radio = Some(tx);
    rig.connect(1, true);
    rig.drain();
    while radio.try_recv().is_ok() {}

    rig.p.input(press("KeyN"));
    assert_eq!(rig.drain(), [stack(1, StackCmd::Media(media_button::NEXT))]);

    rig.hub.update(|s| {
        s.dab.active = true;
        s.dab.playing = true;
    });
    rig.p.input(press("KeyN"));
    assert_eq!(rig.drain(), [stack(1, StackCmd::Media(media_button::NEXT))]);
    assert!(
        radio.try_recv().is_err(),
        "the iPhone steers the radio as its own source"
    );
}

#[tokio::test]
async fn keys_follow_the_bindings() {
    let mut rig = Rig::new();
    rig.p.input(press("KeyN"));
    assert!(rig.drain().is_empty());
    rig.connect(1, true);
    rig.drain();

    for code in [
        "ArrowLeft",
        "Digit1",
        "KeyN",
        "KeyA",
        "KeyV",
        "KeyH",
        "KeyQ",
    ] {
        rig.p.input(press(code));
    }
    rig.p.input(Input::Key {
        code: "KeyV".into(),
        down: false,
    });
    assert_eq!(
        rig.drain(),
        [
            stack(1, StackCmd::Media(media_button::NEXT)),
            stack(1, StackCmd::Telephony(telephony_button::HOOK_SWITCH)),
            stack(1, StackCmd::Siri),
            stack(1, home()),
        ]
    );

    rig.want(Front::Projection);
    rig.drain();
    let none = KnobState::default();
    let mut cfg = rig.hub.config();
    let b = &mut cfg.bindings;
    for (field, code) in [
        (&mut b.knob_left, "KeyJ"),
        (&mut b.knob_right, "KeyK"),
        (&mut b.knob_up, "KeyU"),
        (&mut b.knob_down, "KeyI"),
        (&mut b.select_up, "KeyE"),
        (&mut b.play, "KeyX"),
        (&mut b.pause, "KeyY"),
        (&mut b.phone_key_star, "KeyM"),
        (&mut b.phone_key_hash, "KeyG"),
        (&mut b.phone_key_hook_switch, "KeyO"),
        (&mut b.voice_assistant_release, "KeyW"),
        (&mut b.cluster_content, "KeyZ"),
        (&mut b.cluster_layout, "KeyC"),
    ] {
        *field = code.into();
    }
    rig.p.on_config(&cfg);
    rig.drain();
    for (code, cmd) in [
        (
            "ArrowLeft",
            knob(KnobState {
                x: -KNOB_DEFLECT,
                ..none
            }),
        ),
        (
            "ArrowRight",
            knob(KnobState {
                x: KNOB_DEFLECT,
                ..none
            }),
        ),
        (
            "ArrowUp",
            knob(KnobState {
                y: -KNOB_DEFLECT,
                ..none
            }),
        ),
        (
            "ArrowDown",
            knob(KnobState {
                y: KNOB_DEFLECT,
                ..none
            }),
        ),
        ("Backspace", knob(KnobState { back: true, ..none })),
        ("KeyH", home()),
        (
            "KeyJ",
            knob(KnobState {
                wheel: -1.0,
                ..none
            }),
        ),
        ("KeyK", knob(KnobState { wheel: 1.0, ..none })),
        ("KeyU", StackCmd::KnobSelect(false)),
        ("KeyI", StackCmd::KnobSelect(true)),
        ("KeyE", StackCmd::KnobSelect(false)),
        ("KeyP", StackCmd::Media(media_button::PLAY_PAUSE)),
        ("KeyX", StackCmd::Media(media_button::PLAY)),
        ("KeyY", StackCmd::Media(media_button::PAUSE)),
        ("KeyB", StackCmd::Media(media_button::PREV)),
        ("KeyR", StackCmd::Telephony(telephony_button::DROP)),
        ("Digit0", StackCmd::Telephony(telephony_button::KEY0)),
        ("Digit9", StackCmd::Telephony(telephony_button::KEY0 + 9)),
        ("KeyM", StackCmd::Telephony(telephony_button::STAR)),
        ("KeyG", StackCmd::Telephony(telephony_button::POUND)),
        ("KeyO", StackCmd::Telephony(telephony_button::HOOK_SWITCH)),
        ("KeyZ", StackCmd::NextClusterContent),
    ] {
        rig.p.input(press(code));
        assert_eq!(rig.drain(), [stack(1, cmd)], "{code}");
    }
    rig.p.input(press("KeyC"));
    assert_eq!(rig.hub.watch().borrow().cluster.layout, "livi.navMini");
    rig.p.input(press("KeyS"));
    rig.p.input(press("KeyW"));
    assert!(rig.drain().is_empty());
}

#[tokio::test]
async fn the_dashboard_offers_the_layouts_as_arranged() {
    let mut rig = Rig::new();
    let mut cfg = rig.hub.config();
    let off = livi_core_proto::theme::LayoutSlot { on: false, pos: 1 };
    cfg.dashboard_layouts = [("livi.navMini".to_string(), off)].into_iter().collect();
    rig.p.on_state(cfg, Map::new());
    let shown = rig.hub.watch().borrow().cluster.shown.clone();
    assert_eq!(
        shown,
        [
            "livi.strip",
            "livi.navFull",
            "livi.clusterFull",
            "livi.clusterOnly"
        ]
    );

    rig.p
        .arrange_dashboard(rig.p.cluster.moved("livi.clusterOnly", -1));
    let patch = rig.patches.try_recv().unwrap();
    assert_eq!(
        patch["dashboardLayouts"]["livi.clusterOnly"],
        serde_json::json!({ "on": true, "pos": 4 })
    );
    rig.p
        .arrange_dashboard(rig.p.cluster.moved("livi.clusterOnly", 9));
    assert!(
        rig.patches.try_recv().is_err(),
        "nothing to write past the end"
    );
}

#[tokio::test]
async fn a_kept_car_maker_theme_runs_the_cluster() {
    let mut rig = Rig::new();
    let theme = Theme {
        id: "ABC:7:19.0".into(),
        ..livi_theme()
    };
    std::fs::create_dir_all(rig.dir.0.join("themes")).unwrap();
    let kept = serde_json::to_string(&theme).unwrap();
    std::fs::write(rig.dir.0.join("themes/ABC-7.json"), kept).unwrap();
    let running = |rig: &Rig| rig.hub.watch().borrow().cluster.theme.id.clone();

    rig.settings(|c| c.car_play_theme = "ABC:7:19.0".into());
    assert_eq!(running(&rig), "ABC:7:19.0");
    rig.p.theme_fetched(Theme {
        id: "XYZ:1".into(),
        ..livi_theme()
    });
    assert_eq!(
        running(&rig),
        "ABC:7:19.0",
        "a theme fetched for another id is dropped"
    );
    let palettes = vec![livi_core_proto::theme::Palette {
        id: "default".into(),
        accent_dark: "#ffffffff".into(),
        accent_light: "#000000ff".into(),
    }];
    rig.p.theme_fetched(Theme {
        palettes: palettes.clone(),
        ..theme
    });
    assert_eq!(rig.hub.watch().borrow().cluster.theme.palettes, palettes);

    rig.settings(|c| c.car_play_theme = String::new());
    assert_eq!(running(&rig), "livi");
}

#[tokio::test]
async fn the_cluster_follows_the_phone_and_tells_it_livis_own_choice() {
    let mut rig = Rig::new();
    rig.connect(1, true);
    rig.drain();
    rig.p.input(press("KeyT"));
    assert!(
        rig.drain().is_empty(),
        "the phone draws no theme, it gets no layout"
    );
    rig.settings(|c| c.car_play_theme = "livi".into());
    rig.drain();
    let cluster = |rig: &Rig| rig.hub.watch().borrow().cluster.clone();
    rig.p.on_event(CpEvent::Cluster {
        id: 1,
        choice: PhoneChoice::Layout("livi.clusterFull".into()),
    });
    assert_eq!(cluster(&rig).layout, "livi.clusterFull");
    let dark = PhoneChoice::Appearance(Appearance::Dark);
    rig.p.on_event(CpEvent::Cluster {
        id: 1,
        choice: dark,
    });
    assert_eq!(cluster(&rig).appearance, AppearancePreference::Dark);
    assert!(
        rig.drain().is_empty(),
        "the phone's own choice goes nowhere"
    );

    rig.p.on_event(CpEvent::Cluster {
        id: 9,
        choice: PhoneChoice::Layout("livi.navMini".into()),
    });
    assert_eq!(
        cluster(&rig).layout,
        "livi.clusterFull",
        "only the session in front counts"
    );

    rig.p.input(press("KeyT"));
    assert_eq!(cluster(&rig).layout, "livi.clusterOnly");
    let to_phone = StackCmd::ClusterLayout(ui_sync::Layout {
        id: "livi.clusterOnly".into(),
        palette: "default".into(),
        active: vec![("map".into(), "map".into())],
    });
    assert_eq!(rig.drain(), [stack(1, to_phone)]);
}

#[tokio::test]
async fn a_select_key_clicks() {
    let mut rig = Rig::new();
    rig.connect(1, true);
    rig.want(Front::Projection);
    rig.drain();
    rig.p.input(press("Enter"));
    assert_eq!(rig.drain(), [stack(1, StackCmd::KnobSelect(true))]);
    tokio::time::sleep(SELECT_RELEASE * 2).await;
    assert_eq!(rig.drain(), [stack(1, StackCmd::KnobSelect(false))]);
}

#[tokio::test]
async fn next_device_goes_round_the_phones() {
    let mut rig = Rig::new();
    rig.connect(1, false);
    rig.p.activate_next();
    assert!(rig.p.sessions[0].active);
    rig.device(2, false, "AA:BB", "cc:dd");
    rig.p.activate_next();
    assert!(!rig.p.sessions[0].active && rig.p.sessions[1].active);
    assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 2, 2));
    rig.p.activate_next();
    assert!(rig.p.sessions[0].active);
}

#[tokio::test]
async fn a_call_or_siri_brings_the_projection_forward_and_back() {
    let mut rig = Rig::new();
    rig.connect(1, true);
    rig.drain();
    assert_eq!(rig.front(), Front::Livi);

    rig.p.on_event(CpEvent::Call {
        id: 1,
        phase: CallPhase::Ringing,
    });
    assert_eq!(rig.front(), Front::Projection);
    rig.p.asked(PerScreen {
        cluster: Front::Cluster,
        ..livi()
    });
    assert_eq!(rig.front(), Front::Projection);
    rig.p.asked(PerScreen {
        main: Front::Projection,
        cluster: Front::Cluster,
        aux: Front::Livi,
    });
    assert_eq!(
        rig.drain(),
        [CpCmd::ClusterActive(true), stack(1, StackCmd::Keyframe)]
    );

    rig.p.on_event(CpEvent::SpeechActive {
        id: 1,
        active: true,
    });
    rig.p.on_event(CpEvent::Call {
        id: 1,
        phase: CallPhase::Ended,
    });
    assert_eq!(rig.front(), Front::Projection);
    rig.p.on_event(CpEvent::SpeechActive {
        id: 1,
        active: false,
    });
    assert_eq!(rig.front(), Front::Livi);
    rig.want(Front::Livi);
    assert_eq!(rig.front(), Front::Livi);

    let call = AudioProfile {
        kind: AudioKind::Call,
        label: "telephony".into(),
    };
    rig.p.on_event(CpEvent::AudioActive {
        id: 1,
        profile: call.clone(),
        active: true,
    });
    assert_eq!(rig.front(), Front::Projection);
    rig.want(Front::Projection);
    rig.want(Front::Livi);
    assert_eq!(rig.front(), Front::Livi);
    rig.p.on_event(CpEvent::AudioActive {
        id: 1,
        profile: call,
        active: false,
    });
    assert_eq!(rig.front(), Front::Livi);

    rig.want(Front::Projection);
    rig.p.on_event(CpEvent::Call {
        id: 1,
        phase: CallPhase::Active,
    });
    rig.p.on_event(CpEvent::Call {
        id: 1,
        phase: CallPhase::Ended,
    });
    assert_eq!(rig.front(), Front::Projection);
}

#[tokio::test]
async fn only_the_phone_in_front_moves_the_screen() {
    let mut rig = Rig::new();
    rig.connect(1, false);
    rig.device(2, false, "AA:BB", "cc:dd");
    rig.p.on_event(CpEvent::Call {
        id: 2,
        phase: CallPhase::Ringing,
    });
    rig.p.on_event(CpEvent::SpeechActive {
        id: 2,
        active: true,
    });
    assert_eq!(rig.front(), Front::Livi);
    rig.want(Front::Projection);
    rig.p.on_event(CpEvent::HostUiRequested { id: 2 });
    assert_eq!(rig.front(), Front::Projection);

    rig.want(Front::Livi);
    rig.p.on_event(CpEvent::SpeechActive {
        id: 1,
        active: true,
    });
    assert_eq!(rig.front(), Front::Projection);
    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    assert_eq!(rig.front(), Front::Livi);
}

#[tokio::test]
async fn the_phones_climate_lands_in_the_telemetry() {
    let mut rig = Rig::new();
    let climate = json!({ "auto": false }).as_object().cloned().unwrap();
    rig.p.on_event(CpEvent::Climate { id: 1, climate });
    assert_eq!(
        rig.telemetry.try_recv(),
        Ok(json!({ "climate": { "auto": false, "source": "carplay" } }))
    );
}

#[tokio::test]
async fn a_closed_notification_lands_in_the_telemetry() {
    let mut rig = Rig::new();
    rig.p.on_event(CpEvent::NotificationClosed { id: 1 });
    assert_eq!(
        rig.telemetry.try_recv(),
        Ok(json!({ "notification": { "open": false } }))
    );
}

#[tokio::test]
async fn now_playing_follows_the_phone_in_front() {
    let mut rig = Rig::new();
    rig.connect(1, false);
    rig.device(2, false, "AA:BB", "cc:dd");
    rig.meta(
        1,
        json!({ "type": "nowplaying", "title": "One", "artist": "A", "app": "Music",
                "durationMs": 1000, "elapsedMs": 10, "playing": true }),
    );
    rig.meta(
        1,
        json!({ "type": "nowplaying", "elapsedMs": 20, "playing": false }),
    );
    rig.meta(1, json!({ "type": "nowplaying", "artwork": "AAAA" }));
    rig.meta(1, json!({ "type": "nowplaying", "artwork": "" }));
    rig.meta(2, json!({ "type": "nowplaying", "title": "Two" }));
    rig.meta(9, json!({ "type": "nowplaying", "title": "Nobody" }));
    assert_eq!(
        rig.now_playing(),
        NowPlaying {
            title: Some("One".into()),
            artist: Some("A".into()),
            album: None,
            app: Some("Music".into()),
            duration_ms: Some(1000.0),
            elapsed_ms: Some(20.0),
            playing: Some(false),
            artwork: Some("AAAA".into()),
        }
    );

    rig.p.activate_next();
    assert_eq!(rig.now_playing().title.as_deref(), Some("Two"));
    rig.p.on_event(CpEvent::Disconnected { id: 2 });
    assert_eq!(rig.now_playing().title.as_deref(), Some("One"));
    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    assert_eq!(rig.now_playing(), NowPlaying::default());
}

#[tokio::test]
async fn navigation_follows_the_phone_in_front_and_clears_at_the_end() {
    let mut rig = Rig::new();
    rig.connect(1, false);
    rig.device(2, false, "AA:BB", "cc:dd");
    rig.meta(
        1,
        json!({ "type": "navigation", "status": 1, "orderType": 2, "roadName": "Main St",
"destinationName": "Home",
                "timeToDestination": 600, "distanceToDestination": 5000,
                "remainDistance": 200, "etaEpoch": 1_800_000_000,
                "turn": { "maneuverType": 1, "turnSide": 0, "junctionType": 1,
                          "turnAngle": -90, "afterRoadName": "Side St" },
                "maneuvers": [
                    { "index": 0, "maneuverType": 1, "afterRoadName": "Side St" },
                    { "index": 1, "maneuverType": 12 }
                ] }),
    );
    rig.meta(
        1,
        json!({ "type": "navigation", "remainDistance": 150, "etaEpoch": 0 }),
    );
    rig.meta(
        2,
        json!({ "type": "navigation", "status": 1, "roadName": "Elsewhere" }),
    );
    assert_eq!(
        rig.navigation(),
        Navigation {
            active: Some(true),
            order_type: Some(2),
            road_name: Some("Main St".into()),
            after_road_name: Some("Side St".into()),
            destination_name: Some("Home".into()),
            time_to_destination: Some(600.0),
            distance_to_destination: Some(5000.0),
            remain_distance: Some(150.0),
            maneuver_type: Some(1),
            turn_side: Some(0),
            junction_type: Some(1),
            turn_angle: Some(-90),
            eta: Some(1_800_000_000.0),
            maneuver_text: Some("Turn left".into()),
            maneuver_distance_text: Some("150 m".into()),
            destination_distance_text: Some("5.0 km".into()),
            time_left_text: Some("10 min".into()),
            maneuvers: vec![
                Maneuver {
                    index: 0,
                    maneuver_type: Some(1),
                    after_road_name: Some("Side St".into()),
                    maneuver_text: Some("Turn left".into()),
                    ..Default::default()
                },
                Maneuver {
                    index: 1,
                    maneuver_type: Some(12),
                    maneuver_text: Some("Arrived".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    );
    let mut cfg = rig.hub.config();
    cfg.language = "de".into();
    rig.p.on_state(cfg, Map::new());
    assert_eq!(
        rig.navigation().maneuver_text.as_deref(),
        Some("Links abbiegen")
    );
    assert_eq!(
        rig.navigation().maneuvers[1].maneuver_text.as_deref(),
        Some("Angekommen")
    );

    rig.meta(
        1,
        json!({ "type": "navigation", "status": 0, "roadName": "Last" }),
    );
    assert_eq!(
        rig.navigation(),
        Navigation {
            active: Some(false),
            road_name: Some("Last".into()),
            ..Default::default()
        }
    );
    rig.p.activate_next();
    assert_eq!(rig.navigation().road_name.as_deref(), Some("Elsewhere"));
    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    rig.p.on_event(CpEvent::Disconnected { id: 2 });
    assert_eq!(rig.navigation(), Navigation::default());
}

#[tokio::test]
async fn the_cluster_streams_while_a_screen_shows_it() {
    let mut rig = Rig::new();
    rig.p.asked(PerScreen {
        cluster: Front::Cluster,
        ..livi()
    });
    rig.p.asked(PerScreen {
        main: Front::Cluster,
        cluster: Front::Cluster,
        aux: Front::Livi,
    });
    assert_eq!(rig.drain(), [CpCmd::ClusterActive(true)]);
    assert_eq!(rig.hub.watch().borrow().front.cluster, Front::Cluster);
    rig.p.asked(PerScreen {
        cluster: Front::Cluster,
        ..livi()
    });
    rig.p.asked(livi());
    assert_eq!(rig.drain(), [CpCmd::ClusterActive(false)]);
}

#[tokio::test]
async fn applying_settings_restarts_the_helper_and_the_sessions() {
    let mut rig = Rig::new();
    rig.connect(1, true);
    rig.drain();
    let restarted = rig.p.helper_restart.clone();
    rig.settings(|c| c.wireless_aa_enabled = true);
    rig.drain();
    assert!(rig.hub.apply_pending());
    rig.p.apply_settings();
    tokio::time::timeout(std::time::Duration::from_secs(1), restarted.notified())
        .await
        .unwrap();
    assert_eq!(rig.drain(), [CpCmd::DropSessions]);
    assert!(!rig.hub.apply_pending());
    assert_eq!(rig.p.apply_due, None);
}

async fn restarted(rig: &Rig) -> bool {
    let restart = rig.p.helper_restart.clone();
    tokio::time::timeout(Duration::from_millis(50), restart.notified())
        .await
        .is_ok()
}

#[tokio::test]
async fn with_no_phone_a_setting_goes_live_once_the_settings_sit_still() {
    let mut rig = Rig::new();
    rig.settings(|c| c.wireless_aa_enabled = true);
    rig.drain();
    assert!(rig.p.apply_due.is_some());
    rig.p.apply_idle();
    assert!(!rig.hub.apply_pending());
    assert!(restarted(&rig).await);
    assert!(rig.drain().is_empty());

    rig.settings(|c| c.audio_volume = 0.3);
    rig.p.apply_idle();
    assert!(!rig.hub.apply_pending());
    assert!(!restarted(&rig).await);
}

#[tokio::test]
async fn with_a_phone_a_setting_waits_until_the_last_phone_leaves() {
    let mut rig = Rig::new();
    rig.connect(1, true);
    rig.settings(|c| c.wireless_aa_enabled = true);
    assert_eq!(rig.p.apply_due, None);
    rig.p.apply_idle();
    assert!(rig.hub.apply_pending());
    assert!(!restarted(&rig).await);

    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    assert!(rig.p.apply_due.is_some());
    rig.p.apply_idle();
    assert!(!rig.hub.apply_pending());
    assert!(restarted(&rig).await);
}

fn listed(rig: &Rig) -> Vec<livi_core_proto::state::DeviceView> {
    rig.hub.watch().borrow().devices.clone()
}

#[tokio::test]
async fn the_phones_seen_make_the_device_list() {
    use livi_core_proto::state::DeviceStatus;

    let mut rig = Rig::new();
    rig.connect(1, false);
    rig.device(2, false, "AA:BB:CC:DD:EE:02", "cc:dd:ee:ff:00:02");
    let list = listed(&rig);
    assert_eq!(list.len(), 1);
    assert_eq!(
        (list[0].id.as_str(), list[0].status),
        ("aa:bb:cc:dd:ee:02", DeviceStatus::Available)
    );
    assert_eq!(
        (list[0].session, list[0].last_transport.as_deref()),
        (Some(2), Some("wifi"))
    );

    rig.p.on_event(CpEvent::Presence {
        id: 2,
        wired: false,
        presence: Presence::Status {
            battery_level: Some(50.0),
            battery_charging: Some(true),
            signal_strength: None,
            carrier_name: None,
        },
    });
    assert_eq!(listed(&rig)[0].battery_level, Some(50.0));

    rig.p
        .on_event(CpEvent::HelperPresence(HelperPresence::Device {
            ids: Ids {
                bt_mac: Some("aa:bb:cc:dd:ee:03".into()),
                usb_udid: Some("u3".into()),
                ..Default::default()
            },
            name: Some("Cable".into()),
        }));
    let cable = listed(&rig)
        .into_iter()
        .find(|d| d.name.as_deref() == Some("Cable"))
        .unwrap();
    assert_eq!(
        (cable.status, cable.last_transport.as_deref()),
        (DeviceStatus::Offline, Some("usb"))
    );
    rig.p
        .on_event(CpEvent::HelperPresence(HelperPresence::Wifi {
            wifi_mac: "cc:dd:ee:ff:00:02".into(),
            ip: "10.0.0.2".into(),
            connected: true,
        }));

    rig.p.select_device("aa:bb:cc:dd:ee:02");
    assert!(rig.p.sessions[1].active);
    assert_eq!(listed(&rig)[0].status, DeviceStatus::Active);
    rig.p.select_device("nobody");

    rig.drain();
    rig.p.forget_device("aa:bb:cc:dd:ee:02");
    assert!(listed(&rig).iter().all(|d| d.id != "aa:bb:cc:dd:ee:02"));
    tokio::time::sleep(FORGET_GRACE + std::time::Duration::from_millis(100)).await;
    assert!(rig.drain().contains(&CpCmd::Close { id: 2 }));
    rig.p.forget_device("aa:bb:cc:dd:ee:02");

    rig.p.on_event(CpEvent::Disconnected { id: 2 });
    rig.p.on_event(CpEvent::HelperConnected);
    rig.p
        .on_event(CpEvent::HelperPresence(HelperPresence::Wifi {
            wifi_mac: "cc:dd:ee:ff:00:09".into(),
            ip: String::new(),
            connected: false,
        }));
}

#[tokio::test]
async fn the_camera_comes_to_the_front_while_reversing() {
    let mut rig = Rig::new();
    let mut cfg = rig.p.config_seen.clone();
    cfg.camera_id = "camera-1".into();
    cfg.camera.main = true;
    let car = |v: Value| v.as_object().cloned().unwrap();
    rig.p.on_state(cfg.clone(), car(json!({ "gear": "R" })));
    assert_eq!(rig.front(), Front::Livi, "no camera seen yet");
    rig.p.camera_found(true);
    assert_eq!(rig.front(), Front::Camera);
    rig.p.on_state(cfg.clone(), car(json!({ "gear": "D" })));
    assert_eq!(rig.front(), Front::Livi);

    rig.connect(1, true);
    rig.p.on_event(CpEvent::Call {
        id: 1,
        phase: CallPhase::Ringing,
    });
    assert_eq!(rig.front(), Front::Projection);
    rig.p
        .on_state(cfg.clone(), car(json!({ "gear": "D", "reverse": true })));
    assert_eq!(rig.front(), Front::Camera, "the camera goes over a call");
    rig.want(Front::Livi);
    rig.p
        .on_state(cfg.clone(), car(json!({ "gear": "D", "reverse": false })));
    assert_eq!(
        rig.front(),
        Front::Projection,
        "the call comes back, the camera page did not end it"
    );

    cfg.auto_switch_on_reverse = false;
    rig.p.on_state(cfg.clone(), car(json!({ "gear": -1 })));
    assert_eq!(rig.front(), Front::Projection);
    assert_eq!(reversing(&car(json!({ "gear": "x" }))), Some(false));
    assert_eq!(reversing(&car(json!({ "speedKph": 3 }))), None);
}

#[tokio::test]
async fn the_phone_hands_the_screen_back() {
    let mut rig = Rig::new();
    rig.connect(1, true);
    rig.want(Front::Projection);
    rig.p.on_event(CpEvent::HostUiRequested { id: 1 });
    assert_eq!(rig.front(), Front::Livi);

    rig.p.on_event(CpEvent::SpeechActive {
        id: 1,
        active: true,
    });
    assert_eq!(rig.front(), Front::Projection);
    rig.p.on_event(CpEvent::HostUiRequested { id: 1 });
    assert_eq!(rig.front(), Front::Livi);
    rig.p.on_event(CpEvent::SpeechActive {
        id: 1,
        active: false,
    });
    assert_eq!(rig.front(), Front::Livi);
}

#[tokio::test]
async fn only_the_config_sets_the_night_modes() {
    let mut rig = Rig::new();
    let cfg = rig.p.config_seen.clone();
    let snap = |v: Value| v.as_object().cloned().unwrap_or_default();
    rig.p
        .on_state(cfg.clone(), snap(json!({ "nightMode": true, "ts": 1 })));
    rig.p
        .on_state(cfg.clone(), snap(json!({ "nightMode": true, "ts": 2 })));
    assert_eq!(
        rig.drain(),
        [],
        "telemetry reaches carplay through the config"
    );

    let mut day = cfg.clone();
    day.appearance_mode = AppearanceMode::Day;
    rig.p
        .on_state(day, snap(json!({ "nightMode": true, "ts": 2 })));
    let levels = stream_volumes(
        cfg.audio_volume,
        cfg.nav_volume,
        cfg.call_volume,
        cfg.voice_assistant_volume,
    );
    assert_eq!(rig.drain(), [CpCmd::NightModes([true; 4]), levels.clone()]);

    rig.p.on_event(CpEvent::Spawned { id: 1 });
    assert_eq!(rig.drain(), [levels], "a new session hears the volumes");
}

fn aa(id: AaId, cmd: SessionCmd) -> AaCmd {
    AaCmd::Session { id, cmd }
}

fn command(id: AaId, c: Command) -> AaCmd {
    aa(id, SessionCmd::Command(c))
}

impl Rig {
    fn aa_connect(&mut self, id: AaId, wired: bool) {
        let usb_serial = wired.then(|| format!("serial{id}"));
        self.p.on_aa(AaEvent::Spawned {
            id,
            wired,
            usb_serial,
            peer: String::new(),
        });
        self.p.on_aa(AaEvent::Connected { id });
        self.p.main_video_shown();
    }
}

#[tokio::test]
async fn android_auto_takes_the_screen_like_carplay() {
    let mut rig = Rig::new();
    rig.aa_connect(5, true);
    assert_eq!(rig.sessions(), (Some(Protocol::Androidauto), 1, 1));
    assert_eq!(rig.p.sessions[0].transport, Transport::Usb);
    let sent = rig.aa_drain();
    assert!(sent.contains(&aa(5, SessionCmd::VideoActive(true))));
    assert!(sent.contains(&aa(5, SessionCmd::Keyframe)));
    assert!(sent.iter().any(|c| matches!(
        c,
        AaCmd::Session {
            id: 5,
            cmd: SessionCmd::StreamVolume {
                kind: AaKind::Media,
                ..
            }
        }
    )));
    assert!(rig.drain().is_empty());

    rig.want(Front::Projection);
    assert_eq!(
        rig.aa_drain(),
        [command(5, Command::Home), aa(5, SessionCmd::Keyframe)]
    );
    rig.p.on_aa(AaEvent::Duck {
        id: 5,
        level: 0.2,
        duration_ms: 300,
    });
    rig.p.on_aa(AaEvent::Audio {
        id: 5,
        stream: livi_aa_stack::channels::audio::AudioChannelType::Speech,
        sample_rate: 16000,
        channels: 1,
        active: true,
    });
    let levels = rig.aa_drain();
    assert!(matches!(
        levels.as_slice(),
        [
            AaCmd::Session {
                id: 5,
                cmd: SessionCmd::StreamVolume {
                    kind: AaKind::Media,
                    ramp_ms: 300,
                    ..
                }
            },
            AaCmd::Session {
                id: 5,
                cmd: SessionCmd::StreamVolume {
                    kind: AaKind::Alert,
                    ramp_ms: 0,
                    ..
                }
            }
        ]
    ));

    rig.p.on_aa(AaEvent::HostUiRequested { id: 5 });
    assert_eq!(rig.front(), Front::Livi);
    rig.p.on_aa(AaEvent::Disconnected { id: 5 });
    assert_eq!(rig.sessions(), (None, 0, 0));
    rig.p.on_aa(AaEvent::Connected { id: 9 });
    assert_eq!(rig.p.sessions[0].transport, Transport::Wifi);
}

#[tokio::test]
async fn carplay_and_android_auto_share_the_sessions() {
    let mut rig = Rig::new();
    rig.connect(1, true);
    rig.aa_connect(5, false);
    assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 2));
    rig.drain();
    rig.aa_drain();

    rig.p.activate_next();
    assert_eq!(rig.sessions(), (Some(Protocol::Androidauto), 2, 2));
    assert!(
        rig.drain()
            .contains(&stack(1, StackCmd::VideoActive(false)))
    );
    assert!(
        rig.aa_drain()
            .contains(&aa(5, SessionCmd::VideoActive(true)))
    );

    rig.p.on_aa(AaEvent::Disconnected { id: 5 });
    assert_eq!(rig.sessions(), (Some(Protocol::Carplay), 1, 1));
    assert!(rig.drain().contains(&stack(1, StackCmd::VideoActive(true))));
    assert_eq!(rig.aa_drain(), [aa(5, SessionCmd::VideoActive(false))]);
}

#[tokio::test]
async fn android_auto_touches_land_on_its_stream_and_the_voice_key_is_held() {
    let mut rig = Rig::new();
    rig.aa_connect(5, true);
    rig.want(Front::Projection);
    rig.aa_drain();
    rig.crop.send_replace(Some(Crop {
        crop_l: 0.0,
        crop_t: 60.0,
        vis_w: 1280.0,
        vis_h: 600.0,
        tier_w: 1280.0,
        tier_h: 720.0,
    }));
    rig.p.input(Input::Pointer {
        screen: Screen::Main,
        points: vec![
            Point {
                id: 3,
                x: 0.5,
                y: 0.0,
                phase: Phase::Down,
            },
            Point {
                id: 4,
                x: 1.0,
                y: 1.0,
                phase: Phase::Cancel,
            },
            Point {
                id: 5,
                x: 0.0,
                y: 0.5,
                phase: Phase::Move,
            },
        ],
    });
    assert_eq!(
        rig.aa_drain(),
        [aa(
            5,
            SessionCmd::MultiTouch(vec![
                TouchItem {
                    id: 3,
                    x: 0.5,
                    y: 60.0 / 720.0,
                    action: TouchAction::Down
                },
                TouchItem {
                    id: 4,
                    x: 1.0,
                    y: 660.0 / 720.0,
                    action: TouchAction::Up
                },
                TouchItem {
                    id: 5,
                    x: 0.0,
                    y: 360.0 / 720.0,
                    action: TouchAction::Move
                },
            ])
        )]
    );

    rig.p.input(press("KeyV"));
    rig.p.input(Input::Key {
        code: "KeyV".into(),
        down: false,
    });
    rig.p.input(Input::Key {
        code: "KeyN".into(),
        down: false,
    });
    for code in [
        "Digit7",
        "Digit0",
        "KeyQ",
        "ArrowLeft",
        "ArrowRight",
        "ArrowUp",
        "ArrowDown",
    ] {
        rig.p.input(press(code));
    }
    for code in [
        "Backspace",
        "KeyH",
        "KeyP",
        "KeyN",
        "KeyB",
        "KeyA",
        "KeyR",
        "KeyS",
    ] {
        rig.p.input(press(code));
    }
    rig.p.media(MediaControl::Pause);
    assert_eq!(
        rig.aa_drain(),
        [
            command(5, Command::VoiceAssistant),
            command(5, Command::VoiceAssistantRelease),
            command(5, Command::PhoneKey7),
            command(5, Command::PhoneKey0),
            command(5, Command::Left),
            command(5, Command::Right),
            command(5, Command::Up),
            command(5, Command::Down),
            command(5, Command::Back),
            command(5, Command::Home),
            command(5, Command::PlayPause),
            command(5, Command::Next),
            command(5, Command::Prev),
            command(5, Command::AcceptPhone),
            command(5, Command::RejectPhone),
            command(5, Command::Pause),
        ]
    );

    rig.p.input(press("Enter"));
    assert_eq!(rig.aa_drain(), [command(5, Command::SelectDown)]);
    tokio::time::sleep(SELECT_RELEASE * 2).await;
    assert_eq!(rig.aa_drain(), [command(5, Command::SelectUp)]);
}

#[tokio::test]
async fn android_auto_media_and_guidance_reach_the_state_of_the_phone_in_front() {
    use livi_aa_stack::channels::nav_maneuver::{DrivingSide, ManeuverType};

    let mut rig = Rig::new();
    rig.aa_connect(5, true);
    let first = AaNowPlaying {
        title: Some("Song".into()),
        artist: Some("Band".into()),
        duration_ms: Some(200_000),
        playing: Some(true),
        artwork: Some(vec![1, 2, 3]),
        ..Default::default()
    };
    rig.p.on_aa(AaEvent::NowPlaying {
        id: 5,
        now_playing: first,
    });
    let update = AaNowPlaying {
        elapsed_ms: Some(5_000),
        album: Some("Album".into()),
        app: Some("Music".into()),
        artwork: Some(Vec::new()),
        ..Default::default()
    };
    rig.p.on_aa(AaEvent::NowPlaying {
        id: 5,
        now_playing: update,
    });
    let np = rig.now_playing();
    assert_eq!(
        (np.title.as_deref(), np.album.as_deref()),
        (Some("Song"), Some("Album"))
    );
    assert_eq!(
        (np.duration_ms, np.elapsed_ms, np.playing),
        (Some(200_000.0), Some(5_000.0), Some(true))
    );
    assert_eq!(
        (np.artwork.as_deref(), np.app.as_deref()),
        (Some("AQID"), Some("Music"))
    );

    rig.p.on_aa(AaEvent::NavigationImage {
        id: 5,
        image: vec![9],
    });
    let guidance = AaNavigation {
        active: Some(true),
        app_name: Some("Maps".into()),
        road_name: Some("Main St".into()),
        maneuver_type: Some(ManeuverType::LeftTurn),
        turn_side: Some(DrivingSide::Left),
        turn_angle: Some(90),
        remain_distance: Some(120),
        destination_name: Some("Home".into()),
        distance_to_destination: Some(5_000),
        time_to_destination: Some(600),
        eta: Some("21:58".into()),
        ..Default::default()
    };
    rig.p.on_aa(AaEvent::Navigation {
        id: 5,
        navigation: guidance,
    });
    let nav = rig.navigation();
    assert_eq!(nav.maneuver_type, Some(ManeuverType::LeftTurn as u32));
    assert_eq!(
        (nav.turn_side, nav.turn_angle, nav.remain_distance),
        (Some(1), Some(90), Some(120.0))
    );
    assert_eq!(
        (nav.eta_text.as_deref(), nav.app_name.as_deref()),
        (Some("21:58"), Some("Maps"))
    );
    assert_eq!(
        (nav.image.as_deref(), nav.time_to_destination),
        (Some("CQ=="), Some(600.0))
    );

    rig.p.on_aa(AaEvent::NavigationImage {
        id: 5,
        image: Vec::new(),
    });
    assert_eq!(rig.navigation().image, None);
    let ended = AaNavigation {
        active: Some(false),
        road_name: Some("x".into()),
        ..Default::default()
    };
    rig.p.on_aa(AaEvent::Navigation {
        id: 5,
        navigation: ended,
    });
    assert_eq!(
        rig.navigation(),
        Navigation {
            active: Some(false),
            ..Default::default()
        }
    );

    rig.p.on_aa(AaEvent::NowPlaying {
        id: 7,
        now_playing: AaNowPlaying::default(),
    });
    rig.p.on_aa(AaEvent::Navigation {
        id: 7,
        navigation: AaNavigation::default(),
    });
    rig.p.on_aa(AaEvent::NavigationImage {
        id: 7,
        image: vec![1],
    });
}

#[tokio::test]
async fn what_the_phone_in_front_plays_reaches_the_status_file() {
    let mut rig = Rig::new();
    let (tx, seen) = watch::channel(Activity::default());
    rig.p.activity = Some(tx);
    rig.connect(1, true);
    let nav = AudioProfile {
        kind: AudioKind::Alert,
        label: "alert".into(),
    };
    rig.p.on_event(CpEvent::AudioActive {
        id: 1,
        profile: nav,
        active: true,
    });
    rig.p.on_event(CpEvent::Call {
        id: 1,
        phase: CallPhase::Ringing,
    });
    assert!(seen.borrow().nav_announcing && seen.borrow().speech && seen.borrow().phone);
    rig.p.on_event(CpEvent::Call {
        id: 1,
        phase: CallPhase::Ended,
    });
    assert!(!seen.borrow().phone);

    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    assert_eq!(*seen.borrow(), Activity::default());
    rig.aa_connect(5, false);
    rig.p.on_aa(AaEvent::Audio {
        id: 5,
        stream: livi_aa_stack::channels::audio::AudioChannelType::Media,
        sample_rate: 48000,
        channels: 2,
        active: true,
    });
    rig.p.on_aa(helper(json!({ "event": "sco", "up": true })));
    assert!(seen.borrow().media && seen.borrow().phone);
}

#[tokio::test]
async fn the_goodbye_ends_every_session_and_is_answered_after_the_last() {
    let mut rig = Rig::new();
    let (done, heard) = oneshot::channel();
    rig.p.goodbye(done);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), heard)
            .await
            .unwrap(),
        Ok(())
    );

    rig.connect(1, true);
    rig.aa_connect(5, false);
    rig.drain();
    rig.aa_drain();
    let (done, mut heard) = oneshot::channel();
    rig.p.goodbye(done);
    assert!(rig.drain().contains(&CpCmd::Close { id: 1 }));
    assert!(rig.aa_drain().contains(&aa(5, SessionCmd::PowerDown)));
    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(heard.try_recv().is_err());
    rig.p.on_aa(AaEvent::Disconnected { id: 5 });
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), heard)
            .await
            .unwrap(),
        Ok(())
    );
}

#[tokio::test]
async fn closing_livi_hands_the_helper_an_empty_paging_list_and_nothing_after() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut rig = Rig::new();
    let helper = tokio::net::UnixListener::bind(rig.dir.0.join("cp-bt.sock")).unwrap();
    let (done, heard) = oneshot::channel();
    rig.p.goodbye(done);
    rig.p.publish();

    let (conn, _) = helper.accept().await.unwrap();
    let (rd, mut wr) = conn.into_split();
    let mut line = String::new();
    tokio::io::BufReader::new(rd)
        .read_line(&mut line)
        .await
        .unwrap();
    assert_eq!(line, "reconnect-targets []\n");
    wr.write_all(b"{\"ok\":true}\n").await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), heard)
            .await
            .unwrap(),
        Ok(())
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), helper.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_seek_reaches_the_carplay_phone_in_front_by_its_bluetooth_mac() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut rig = Rig::new();
    let helper = tokio::net::UnixListener::bind(rig.dir.0.join("cp-bt.sock")).unwrap();
    rig.connect(1, false);
    rig.device(1, false, "AA:BB:CC:DD:EE:FF", "");
    rig.drain();
    rig.p.seek(61000);

    let line = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (conn, _) = helper.accept().await.unwrap();
            let (rd, mut wr) = conn.into_split();
            let mut line = String::new();
            tokio::io::BufReader::new(rd)
                .read_line(&mut line)
                .await
                .unwrap();
            wr.write_all(b"{\"ok\":true}\n").await.unwrap();
            if line.starts_with("seek ") {
                return line;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(line, "seek 61000 aa:bb:cc:dd:ee:ff\n");
}

fn aa_device(id: AaId, model: &str, instance: &str) -> AaEvent {
    AaEvent::Device {
        id,
        name: "Android".into(),
        model: model.into(),
        instance_id: instance.into(),
        ip: String::new(),
    }
}

fn helper(ev: Value) -> AaEvent {
    AaEvent::Helper(serde_json::from_value(ev).expect("a helper event"))
}

#[tokio::test]
async fn an_android_auto_call_on_hands_free_comes_to_the_front_and_gets_its_audio() {
    let mut rig = Rig::new();
    rig.aa_connect(5, false);
    assert_eq!(rig.io_drain(), [Io::FindPhones]);
    let sco = |up: bool| helper(json!({ "event": "sco", "up": up }));
    rig.p.on_aa(sco(true));
    rig.p.on_aa(sco(true));
    assert_eq!(rig.io_drain(), [Io::Call(true)]);
    assert_eq!(rig.front(), Front::Projection);
    rig.p.on_aa(sco(false));
    assert_eq!(rig.io_drain(), [Io::Call(false)]);
    assert_eq!(rig.front(), Front::Livi);
}

#[tokio::test]
async fn the_helper_names_the_android_auto_phone_and_its_link_is_kept() {
    use livi_aa_stack::bridge::PhoneStatus;

    let mut rig = Rig::new();
    let mac = "aa:00:00:00:00:05";
    rig.p.on_aa(helper(json!({
        "event": "aa-device", "btMac": "AA:00:00:00:00:05", "instanceId": "inst5", "usbSerial": ""
    })));
    assert_eq!(rig.io_drain(), [Io::Nudge(mac.into())]);
    rig.aa_connect(5, false);
    assert!(rig.io_drain().is_empty());

    rig.p.on_aa(aa_device(5, "Pixel 9", "inst5"));
    let phone = &listed(&rig)[0];
    assert_eq!(
        (phone.id.as_str(), phone.model.as_deref()),
        (mac, Some("Pixel 9"))
    );
    assert_eq!(
        (phone.protocol, phone.session),
        (Some(Protocol::Androidauto), Some(1))
    );

    let battery = |pct: u32| {
        helper(json!({ "event": "phone-battery", "mac": "AA:00:00:00:00:05", "pct": pct }))
    };
    rig.p.on_aa(battery(60));
    assert_eq!(listed(&rig)[0].battery_level, Some(60.0));
    let status = PhoneStatus {
        battery_level: Some(73),
        signal_strength: Some(3),
        ..Default::default()
    };
    rig.p.on_aa(AaEvent::Status { id: 5, status });
    rig.p.on_aa(battery(40));
    let phone = &listed(&rig)[0];
    assert_eq!(
        (phone.battery_level, phone.signal_strength),
        (Some(73.0), Some(3.0))
    );

    rig.p.on_aa(helper(
        json!({ "event": "hfp", "up": true, "mac": "AA:00:00:00:00:05" }),
    ));
    rig.p.keep_links();
    assert!(rig.io_drain().is_empty());
    rig.p.on_aa(helper(json!({ "event": "hfp", "up": false })));
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(121)).await;
    rig.p.keep_links();
    assert_eq!(rig.io_drain(), [Io::Nudge(mac.into())]);
}

#[tokio::test]
async fn the_helper_hears_which_android_auto_phones_are_on_the_cable() {
    let mut rig = Rig::new();
    rig.aa_connect(5, true);
    assert_eq!(
        rig.io_drain(),
        [Io::WiredPhones(vec!["SERIAL5".into()]), Io::FindPhones]
    );
    rig.p.on_aa(aa_device(5, "Pixel", "inst5"));
    let both = vec!["INST5".to_string(), "SERIAL5".into()];
    assert_eq!(rig.io_drain(), [Io::WiredPhones(both.clone())]);
    rig.p.on_aa(AaEvent::HelperConnected);
    assert_eq!(rig.io_drain(), [Io::WiredPhones(both)]);
    rig.p.on_aa(AaEvent::Disconnected { id: 5 });
    assert_eq!(rig.io_drain(), [Io::WiredPhones(Vec::new())]);
}

#[tokio::test]
async fn the_one_connected_phone_is_taken_for_an_unnamed_android_auto_session() {
    let mut rig = Rig::new();
    rig.aa_connect(5, false);
    rig.p.on_aa(aa_device(5, "Pixel", "inst5"));
    assert_eq!(listed(&rig)[0].id, "inst5");
    rig.io_drain();

    rig.p
        .on_back(Back::Phones(vec!["AA:00:00:00:00:07".into()]));
    let named = rig
        .p
        .hands_free
        .bt_by_instance
        .get("inst5")
        .map(String::as_str);
    assert_eq!(named, Some("AA:00:00:00:00:07"));
    assert_eq!(rig.io_drain(), [Io::Nudge("aa:00:00:00:00:07".into())]);
    assert_eq!(listed(&rig)[0].id, "aa:00:00:00:00:07");

    rig.p.on_back(Back::Phones(vec![
        "AA:00:00:00:00:08".into(),
        "AA:00:00:00:00:09".into(),
    ]));
    assert_eq!(rig.p.hands_free.bt_by_instance.len(), 1);
    assert_eq!(rig.io_drain().len(), 2);
}

#[tokio::test]
async fn an_iphone_mac_is_not_taken_for_an_unnamed_android_auto_session() {
    let mut rig = Rig::new();
    rig.connect(1, false);
    rig.device(1, false, "AA:00:00:00:00:07", "");
    rig.aa_connect(5, false);
    rig.p.on_aa(aa_device(5, "Pixel", "inst5"));
    rig.io_drain();

    rig.p
        .on_back(Back::Phones(vec!["AA:00:00:00:00:07".into()]));
    assert!(rig.p.hands_free.bt_by_instance.is_empty());
    let mut ids: Vec<String> = listed(&rig).into_iter().map(|d| d.id).collect();
    ids.sort();
    assert_eq!(ids, ["aa:00:00:00:00:07", "inst5"]);

    rig.p.on_event(CpEvent::Disconnected { id: 1 });
    rig.p
        .on_back(Back::Phones(vec!["AA:00:00:00:00:07".into()]));
    assert!(rig.p.hands_free.bt_by_instance.is_empty());
}

fn airplay(rig: &mut Rig, id: u64, name: &str) -> UnboundedReceiver<AirplayCommand> {
    let (remote, commands) = AirplayRemote::channel();
    rig.p.on_ap(ApEvent::Started {
        id,
        close: Arc::new(Notify::new()),
        peer: AirplaySender {
            name: Some(name.into()),
            model: None,
        },
        remote,
    });
    commands
}

#[tokio::test]
async fn the_mirror_picture_waits_for_park() {
    let mut rig = Rig::new();
    let _commands = airplay(&mut rig, 1, "LaPhone");
    rig.p.video_up(Link::Ap(1));
    let cfg = rig.p.config_seen.clone();
    let car = |v: Value| v.as_object().cloned().unwrap();
    let held = |rig: &Rig| rig.hub.watch().borrow().sessions.held;
    assert!(!held(&rig), "no gear known yet");
    rig.p.on_state(cfg.clone(), car(json!({ "gear": "D" })));
    assert!(held(&rig));
    rig.p.on_state(cfg.clone(), car(json!({ "gear": "P" })));
    assert!(!held(&rig));
}

#[tokio::test]
async fn media_keys_reach_the_airplay_sender() {
    let mut rig = Rig::new();
    let mut commands = airplay(&mut rig, 1, "LaPhone");
    rig.p.media(MediaControl::PlayPause);
    rig.p.on_ap(ApEvent::NowPlaying {
        id: 1,
        patch: NowPlayingPatch {
            playing: Some(true),
            ..Default::default()
        },
    });
    rig.p.media(MediaControl::PlayPause);
    rig.p.media(MediaControl::Next);
    rig.p.media(MediaControl::Prev);
    rig.p.key("KeyN", true);
    rig.p.key("KeyN", false);
    rig.p.seek(61_500);
    let mut sent = Vec::new();
    while let Ok(c) = commands.try_recv() {
        sent.push(c);
    }
    assert_eq!(
        sent,
        [
            AirplayCommand::Play,
            AirplayCommand::Pause,
            AirplayCommand::Next,
            AirplayCommand::Prev,
            AirplayCommand::Next,
            AirplayCommand::Seek(61.5)
        ]
    );
}

#[tokio::test]
async fn a_mirror_shows_what_carplay_on_the_same_phone_plays() {
    let mut rig = Rig::new();
    rig.connect(1, false);
    rig.p.on_event(CpEvent::Presence {
        id: 1,
        wired: false,
        presence: Presence::Device {
            bt_mac: "aa:bb:cc:dd:ee:01".into(),
            wifi_mac: String::new(),
            ip: String::new(),
            usb_udid: String::new(),
            name: "LaPhone".into(),
            model: String::new(),
        },
    });
    rig.meta(1, json!({ "type": "nowplaying", "title": "Car" }));
    airplay(&mut rig, 7, "laphone");
    rig.p.ap_video_shown();
    assert_eq!(rig.sessions().0, Some(Protocol::Airplay));
    assert_eq!(rig.now_playing().title.as_deref(), Some("Car"));
    rig.meta(1, json!({ "type": "nowplaying", "title": "Next" }));
    assert_eq!(rig.now_playing().title.as_deref(), Some("Next"));
}
