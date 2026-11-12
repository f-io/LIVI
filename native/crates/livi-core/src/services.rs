// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::aa_events::{AaHelperEvent, RemoteCommand};
use livi_core_proto::config::Config;
use livi_core_proto::helper::HelperSockets;
use livi_core_proto::message::MediaControl;
use livi_core_proto::state::{Front, Protocol, Screen, State};
use livi_cp::helper_sock::HelperSock;
use livi_cp::pairings::Pairings;
use livi_cp::stack::StackCtx;
use livi_output_proto::{PLANE_AIRPLAY, PLANE_CLUSTER_RECV, PLANE_MAIN};
use livi_session_io::line::Feed;
use tokio::net::TcpListener;
use tokio::process::Child;
use tokio::sync::{Notify, mpsc, oneshot, watch};

use crate::audio::spectrum;
use crate::devices::{Devices, Shared};
use crate::files::paths::Paths;
use crate::helper::{self, Helper, HelperSupervisor, Spec};
use crate::hub::Hub;
use crate::projection::android_auto::{self, GstAaMedia};
use crate::projection::carplay::{ConfigSource, GstMedia};
use crate::projection::{Drivers, Projection, UiAsks, UiCommand};
use crate::resources::{Resources, gst_env};
use crate::ui::UiSupervisor;
use livi_aa_stack::helper_sock::AaHelperSock;
use livi_media::compositor::{self, CompositorControl, Ui, backdrop_hex};
use livi_media::output::{Launch, Output, OutputBackend, OutputEvent, Process};
use livi_media::planes::{ClusterPlanes, MainPlane, VideoPlane, crop_for};

const HELPER_RESTART_DELAY: Duration = Duration::from_secs(2);
const HELPER_MAX_RESTARTS: u32 = 5;
const COMPOSITOR_STOP_GRACE: Duration = Duration::from_secs(3);

pub struct Services {
    helper: Option<HelperSupervisor>,
    compositor: Option<Watched>,
    ui: Option<UiSupervisor>,
}

struct Watched {
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

fn watch_compositor(mut child: Child, quit: Arc<Notify>) -> Watched {
    let (stop, stop_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _lifeline = crate::system::child::hold_lifeline(&mut child);
        tokio::select! {
            status = child.wait() => {
                println!("[core] the compositor closed: {status:?}");
                quit.notify_one();
            }
            _ = stop_rx => {
                crate::system::child::terminate(&mut child, COMPOSITOR_STOP_GRACE, false).await;
            }
        }
    });
    Watched { stop, task }
}

async fn follow_displays(
    mut displays: tokio::sync::watch::Receiver<Option<Vec<livi_core_proto::state::Display>>>,
    hub: Arc<Hub>,
) {
    while displays.changed().await.is_ok() {
        let Some(list) = displays.borrow_and_update().clone() else {
            continue;
        };
        let main = list
            .iter()
            .find(|d| d.screen == Some(livi_core_proto::state::Screen::Main))
            .and_then(|d| Some((d.connector.clone(), d.current.clone()?)));
        hub.update(|s| {
            s.system.displays_settable = true;
            s.system.displays = list;
        });
        let cfg = hub.config();
        let kiosk = cfg.kiosk.main || std::env::var("LIVI_KIOSK").is_ok_and(|v| v == "1");
        if kiosk && let Some((connector, current)) = main {
            crate::audio::host_output::pin(&connector, &current).await;
        }
    }
}

fn crash_log() -> PathBuf {
    let dir = std::env::var_os("APPIMAGE")
        .and_then(|a| Path::new(&a).parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    dir.join("livi-output-crash.log")
}

fn host_launch(res: &Resources, user_data: &Path, ui: Option<&Ui>) -> Option<Launch> {
    let bin = match res.output.clone()? {
        OutputBackend::Process(bin) => bin,
        OutputBackend::Embedded(host) => return Some(Launch::Embedded(host)),
    };
    let mut env = res.gstreamer.as_deref().map(gst_env).unwrap_or_default();
    if cfg!(target_os = "linux") {
        if let Ok(preload) = std::env::var("LIVI_GST_PRELOAD") {
            env.push(("LD_PRELOAD".into(), preload));
        }
        env.push(("GST_GL_WINDOW".into(), "surfaceless".into()));
        env.push(("GST_GL_PLATFORM".into(), "egl".into()));
    }
    if let Some(ui) = ui {
        env.push(("WAYLAND_DISPLAY".into(), ui.wayland_display.clone()));
        if let Some(node) = &ui.render_node {
            env.push(("LIVI_RENDER_NODE".into(), node.clone()));
        }
    }
    Some(Launch::Process(Process {
        bin,
        env,
        log: user_data.join("log/output.log"),
        crash: crash_log(),
    }))
}

impl Services {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        res: Resources,
        paths: &Paths,
        user_data: &Path,
        hub: Arc<Hub>,
        quit: Arc<Notify>,
        mut asks: UiAsks,
        telemetry: mpsc::UnboundedSender<serde_json::Value>,
        settings: mpsc::UnboundedSender<serde_json::Value>,
        resume_radio: Option<crate::radio::Band>,
    ) -> Self {
        let state = hub.watch();
        let cfg = state.borrow().config.clone();
        let identity = livi_cp::identity::load_or_create(&user_data.join("cp/identity.json"));
        let accessory = livi_cp::identity::accessory_id(
            &user_data.join("cp/accessory-id"),
            &identity.pairing_id,
            || crate::system::hwaddr::accessory_id(&cfg.wifi_interface),
        );
        let airplay = carplay_listener();
        let airplay_port = airplay
            .as_ref()
            .map(|l| livi_cp::net::local_port(l.local_addr()));
        let receiver = cfg
            .airplay_enabled
            .then(|| {
                crate::projection::airplay::bind(
                    &identity,
                    &accessory,
                    match cfg.car_play_source_version.trim() {
                        "" => livi_core_proto::config::defaults().car_play_source_version,
                        s => s.to_string(),
                    },
                )
            })
            .flatten();
        let mirror = receiver
            .as_ref()
            .map(crate::projection::airplay::Receiver::mirror);
        let helper_restart = Arc::new(Notify::new());
        let applied = hub.applied();
        let env_identity = identity.clone();
        let env_accessory = accessory.clone();
        let helper = res.helper.clone().map(|helper| {
            let helper = match helper {
                Helper::Process(bin) => Helper::Process(helper::resolve_bin(&bin, user_data)),
                embedded => embedded,
            };
            HelperSupervisor::start(Spec {
                helper,
                start: Box::new(move || {
                    let cfg = applied.borrow().clone();
                    let start =
                        helper::helper_start(&cfg, &env_identity, &env_accessory, airplay_port);
                    livi_core_proto::helper::HelperStart {
                        mirror: mirror.clone().map(|m| livi_core_proto::helper::Mirror {
                            name: livi_core_proto::helper::mirror_name(&start.settings.car_name),
                            ..m
                        }),
                        ..start
                    }
                }),
                restart: helper_restart.clone(),
                use_sudo: cfg!(target_os = "linux"),
                user_data: user_data.to_path_buf(),
                restart_delay: HELPER_RESTART_DELAY,
                max_restarts: Some(HELPER_MAX_RESTARTS),
            })
        });
        if helper.is_none() {
            println!("[core] running without the helper");
        }

        let ctrl_path = paths.runtime.join("compositor.ctrl");
        let compositor = match &res.compositor {
            Some(bin) if cfg!(target_os = "linux") => {
                let started =
                    compositor::start(bin, &cfg, &ctrl_path, &user_data.join("log/compositor.log"));
                if started.is_none() {
                    println!("[core] running without the compositor");
                }
                started
            }
            _ => None,
        };
        let ctrl = CompositorControl::connect(ctrl_path);
        tokio::spawn(follow_displays(ctrl.displays(), hub.clone()));
        let gst = Output::new(paths.runtime.join("gst.sock"));
        if let Some(launch) = host_launch(&res, user_data, None) {
            if compositor.is_some() {
                gst.hold_launch();
            } else {
                gst.set_launch(launch);
            }
        }
        let plane = MainPlane::new(gst.clone(), ctrl.clone());
        let clusters = ClusterPlanes::new(gst.clone(), ctrl.clone());
        let airplay_plane = MainPlane::airplay(gst.clone(), ctrl.clone());
        let (airplay_events_tx, airplay_events) = mpsc::unbounded_channel();
        let airplay_audio = crate::projection::airplay::ApAudio::new(gst.clone());
        if let Some(receiver) = receiver {
            crate::projection::airplay::start(
                receiver,
                user_data,
                &identity,
                &paths.helper.carplay,
                gst.clone(),
                crate::projection::airplay::Airplay {
                    plane: airplay_plane.clone(),
                    audio: airplay_audio.clone(),
                    events: airplay_events_tx,
                    hub: hub.clone(),
                    settings: settings.clone(),
                },
            );
        }

        let (host_ready, host_ready_rx) = watch::channel(compositor.is_none());
        let display = compositor.is_some().then(|| ctrl.ui());
        let ui = res
            .ui
            .clone()
            .map(|cmd| UiSupervisor::start(cmd, paths.socket(), display, quit.clone()));
        if ui.is_none() {
            println!("[core] running without a UI");
        }
        let compositor = compositor.map(|child| watch_compositor(child, quit));
        tokio::spawn(spectrum::follow(
            gst.clone(),
            asks.spectrum.clone(),
            state.clone(),
        ));
        tokio::spawn(crate::vehicle::blinker::run(hub.clone(), gst.clone()));
        tokio::spawn(crate::camera::run(
            hub.clone(),
            gst.clone(),
            livi_media::planes::CameraPlane::new(gst.clone(), ctrl.clone()),
            asks.commands_back.clone(),
            settings.clone(),
            host_ready_rx,
        ));
        tokio::spawn(follow_display_mode(hub.applied(), ctrl.clone()));
        tokio::spawn(forward_touches(
            ctrl.touches(),
            asks.input_from_compositor.clone(),
        ));
        tokio::spawn(follow_fullscreen(
            ctrl.fullscreen_changes(),
            hub.clone(),
            settings.clone(),
        ));
        if let Some(radio_asks) = asks.radio.take() {
            tokio::spawn(crate::radio::run(
                hub.clone(),
                gst.clone(),
                radio_asks,
                asks.activity.subscribe(),
                user_data.to_path_buf(),
                resume_radio,
            ));
        }

        let carplay = CarPlay {
            airplay_events,
            airplay_audio,
            listener: airplay,
            identity,
            accessory,
            user_data: user_data.to_path_buf(),
            sockets: paths.helper.clone(),
            probe: host_launch(&res, user_data, None),
            debug: helper::debug(&cfg),
            helper_restart,
            telemetry,
            settings,
        };
        carplay.start(hub, &ctrl, &gst, plane.clone(), clusters.clone(), asks);

        tokio::spawn(follow_display(
            ctrl.ui(),
            gst.clone(),
            res,
            user_data.to_path_buf(),
            host_ready,
        ));
        tokio::spawn(follow_state(
            state.clone(),
            ctrl,
            plane.clone(),
            airplay_plane.clone(),
            clusters.clone(),
        ));
        tokio::spawn(follow_audio_output(state.clone(), gst.clone()));
        tokio::spawn(follow_host(gst, plane, airplay_plane, clusters, state));

        Self {
            helper,
            compositor,
            ui,
        }
    }

    pub async fn stop(mut self) {
        if let Some(ui) = self.ui.take() {
            ui.stop().await;
        }
        if let Some(compositor) = self.compositor.take() {
            let _ = compositor.stop.send(());
            let _ = compositor.task.await;
        }
        if let Some(helper) = self.helper.take() {
            helper.stop().await;
        }
    }
}

fn carplay_listener() -> Option<TcpListener> {
    match livi_cp::net::tcp_listener() {
        Ok(listener) => {
            let port = livi_cp::net::local_port(listener.local_addr());
            println!("[core] CarPlay listening on :{port} (dual-stack)");
            Some(listener)
        }
        Err(e) => {
            eprintln!("[core] CarPlay cannot listen: {e}");
            None
        }
    }
}

struct CarPlay {
    airplay_events: mpsc::UnboundedReceiver<crate::projection::ApEvent>,
    airplay_audio: Arc<crate::projection::airplay::ApAudio>,
    listener: Option<TcpListener>,
    identity: livi_cp::identity::Identity,
    accessory: String,
    user_data: PathBuf,
    sockets: HelperSockets,
    probe: Option<Launch>,
    debug: bool,
    helper_restart: Arc<Notify>,
    telemetry: mpsc::UnboundedSender<serde_json::Value>,
    settings: mpsc::UnboundedSender<serde_json::Value>,
}

impl CarPlay {
    fn start(
        self,
        hub: Arc<Hub>,
        ctrl: &CompositorControl,
        gst: &Output,
        plane: Arc<MainPlane>,
        clusters: Arc<ClusterPlanes>,
        asks: UiAsks,
    ) {
        let state = hub.watch();
        let (source, config, aa_config) = ConfigSource::new(
            state.clone(),
            ctrl.panels(),
            self.accessory.clone(),
            self.user_data.join("carplay-images"),
        );
        source.follow();
        let probing = source.clone();
        if let Some(probe) = self.probe {
            tokio::spawn(async move {
                let Some(codecs) = livi_media::output::probe_codecs(&probe).await else {
                    return;
                };
                println!("[core] output codecs {codecs}");
                probing.set_codecs(android_auto::codecs(&codecs));
                let _ = tokio::task::spawn_blocking(move || probing.refresh()).await;
            });
        }
        let refreshing = source.clone();
        let ctx = Arc::new(StackCtx {
            identity: self.identity,
            pairings: Pairings::new(self.user_data.join("cp/pairings.json")),
            helper: HelperSock::new(&self.sockets.carplay),
            media: GstMedia::new(gst.clone(), VideoPlane::new(gst.clone(), ctrl.clone())),
            config,
            refresh: Some(Arc::new(move || refreshing.refresh())),
            debug: self.debug,
            video_capture: self
                .debug
                .then(|| self.user_data.join("log/video-playback")),
        });
        let (events_tx, events) = mpsc::unbounded_channel();
        let cp = livi_cp::manager::start(self.listener, ctx, events_tx);
        tokio::spawn(crate::projection::cp_radio::run(hub.clone(), cp.clone()));
        let wants = *asks.wants.borrow();
        let shared = Shared::new(&self.sockets.shared);
        tokio::spawn(remote_input(shared.clone(), asks.commands_back.clone()));
        let devices = Devices::new(
            &self.user_data.join("devices.json"),
            HelperSock::new(&self.sockets.carplay),
            shared.clone(),
        );
        let aa_media = GstAaMedia::new(
            gst.clone(),
            plane.clone(),
            clusters.clone(),
            state.clone(),
            self.sockets.sco_mic.clone(),
        );
        let (aa_events_tx, aa_events) = mpsc::unbounded_channel();
        let aa = livi_aa_stack::manager::start(
            AaHelperSock::new(&self.sockets.android_auto),
            aa_media.clone(),
            aa_config,
            aa_events_tx,
        );
        let (phone_io, phone_asks) = mpsc::unbounded_channel();
        let (phone_answers, phone_back) = mpsc::unbounded_channel();
        tokio::spawn(crate::projection::android_auto::hands_free_io(
            phone_asks,
            aa_media.clone(),
            AaHelperSock::new(&self.sockets.android_auto),
            shared,
            phone_answers,
        ));
        let drivers = Drivers {
            cp,
            aa,
            aa_crop: aa_media.crop(),
            phone_io,
            phone_back,
            telemetry: self.telemetry,
            settings: self.settings,
            themes: self.user_data.join("themes"),
        };
        let mut projection = Projection::new(
            hub,
            drivers,
            plane,
            clusters,
            self.helper_restart,
            devices,
            wants,
        );
        projection.hear_airplay(self.airplay_events, self.airplay_audio);
        tokio::spawn(projection.run(events, aa_events, asks, state));
    }
}

async fn remote_input(shared: Shared, commands: mpsc::UnboundedSender<UiCommand>) {
    let mut feed = shared.subscribe();
    while let Some(item) = feed.recv().await {
        let Feed::Event(event) = item else { continue };
        let command = match serde_json::from_value::<AaHelperEvent>(event) {
            Ok(AaHelperEvent::Input { command }) => command,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("[core] shared sock event not understood: {e}");
                continue;
            }
        };
        match media_control(command) {
            Some(control) => {
                println!("[core] remote input {command:?}");
                let _ = commands.send(UiCommand::Media(control));
            }
            None => println!("[core] remote input {command:?} has no phone counterpart"),
        }
    }
}

fn media_control(command: RemoteCommand) -> Option<MediaControl> {
    match command {
        RemoteCommand::Play => Some(MediaControl::Play),
        RemoteCommand::Pause => Some(MediaControl::Pause),
        RemoteCommand::PlayPause => Some(MediaControl::PlayPause),
        RemoteCommand::Next => Some(MediaControl::Next),
        RemoteCommand::Previous => Some(MediaControl::Prev),
        RemoteCommand::Stop | RemoteCommand::FastForward | RemoteCommand::Rewind => None,
    }
}

async fn follow_display(
    mut ui: watch::Receiver<Option<Ui>>,
    gst: Output,
    res: Resources,
    user_data: PathBuf,
    host_ready: watch::Sender<bool>,
) {
    while ui.changed().await.is_ok() {
        let current = ui.borrow_and_update().clone();
        if let Some(ui) = current {
            println!(
                "[core] compositor display {} on {}",
                ui.wayland_display,
                ui.render_node.as_deref().unwrap_or("-")
            );
            if let Some(launch) = host_launch(&res, &user_data, Some(&ui)) {
                gst.set_launch(launch);
            }
            host_ready.send_replace(true);
        }
    }
}

fn apply_display(cfg: &Config, ctrl: &CompositorControl) {
    ctrl.backdrop(&backdrop_hex(cfg));
    ctrl.gamma(
        cfg.display_gamma,
        cfg.display_contrast,
        cfg.display_color_r,
        cfg.display_color_g,
        cfg.display_color_b,
    );
}

type ScreenWant = (&'static str, bool, Option<(u32, u32)>);

fn secondary_screens(cfg: &Config) -> [ScreenWant; 2] {
    let size = |w: u32, h: u32| (w > 0 && h > 0).then_some((w, h));
    [
        (
            "cluster",
            cfg.cluster_screen_active,
            size(cfg.cluster_screen_width, cfg.cluster_screen_height),
        ),
        (
            "aux",
            cfg.aux_screen_active,
            size(cfg.aux_screen_width, cfg.aux_screen_height),
        ),
    ]
}

fn changed_screens(last: Option<&Config>, now: &Config) -> Vec<ScreenWant> {
    let before = last.map(secondary_screens);
    secondary_screens(now)
        .into_iter()
        .enumerate()
        .filter(|(i, want)| before.is_none_or(|b| b[*i] != *want))
        .map(|(_, want)| want)
        .collect()
}

async fn follow_state(
    mut state: watch::Receiver<State>,
    ctrl: CompositorControl,
    plane: Arc<MainPlane>,
    airplay: Arc<MainPlane>,
    clusters: Arc<ClusterPlanes>,
) {
    let mut last: Option<State> = None;
    loop {
        let now = state.borrow_and_update().clone();
        let display_changed = last.as_ref().is_none_or(|l| {
            l.config.dark_mode != now.config.dark_mode
                || l.config.background_color_dark != now.config.background_color_dark
                || l.config.background_color_light != now.config.background_color_light
                || l.config.display_gamma != now.config.display_gamma
                || l.config.display_contrast != now.config.display_contrast
                || l.config.display_color_r != now.config.display_color_r
                || l.config.display_color_g != now.config.display_color_g
                || l.config.display_color_b != now.config.display_color_b
        });
        if display_changed {
            apply_display(&now.config, &ctrl);
        }
        if last
            .as_ref()
            .is_none_or(|l| l.config.touch_calibration != now.config.touch_calibration)
        {
            ctrl.touch_calibration(&now.config.touch_calibration);
        }
        for (role, on, size) in changed_screens(last.as_ref().map(|l| &l.config), &now.config) {
            ctrl.screen(role, on, size);
        }
        if last
            .as_ref()
            .is_none_or(|l| l.config.kiosk != now.config.kiosk)
        {
            let k = &now.config.kiosk;
            ctrl.fullscreen("main", k.main);
            ctrl.fullscreen("cluster", k.cluster);
            ctrl.fullscreen("aux", k.aux);
        }
        let stream = |c: &Config| (c.projection_width, c.projection_height);
        if last
            .as_ref()
            .is_none_or(|l| stream(&l.config) != stream(&now.config))
        {
            ctrl.aspect("main", Some(stream(&now.config)));
        }
        let shown = |s: &State| {
            (
                s.front.main == Front::Projection,
                s.sessions.active == Some(Protocol::Airplay),
                s.sessions.held,
            )
        };
        if last.as_ref().is_none_or(|l| shown(l) != shown(&now)) {
            let (projection, mirrored, held) = shown(&now);
            plane.set_visible(projection && !mirrored);
            airplay.set_visible(projection && mirrored && !held);
        }
        if last.as_ref().is_none_or(|l| l.front != now.front) {
            let f = &now.front;
            clusters.show_on([f.main, f.cluster, f.aux].map(|f| f == Front::Cluster));
        }
        last = Some(now);
        if state.changed().await.is_err() {
            return;
        }
    }
}

async fn forward_touches(
    mut touches: tokio::sync::broadcast::Receiver<livi_core_proto::input::Input>,
    input: mpsc::UnboundedSender<livi_core_proto::input::Input>,
) {
    loop {
        match touches.recv().await {
            Ok(touch) => {
                if input.send(touch).is_err() {
                    return;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

async fn follow_fullscreen(
    mut changes: tokio::sync::broadcast::Receiver<(Screen, bool)>,
    hub: Arc<Hub>,
    settings: mpsc::UnboundedSender<serde_json::Value>,
) {
    loop {
        match changes.recv().await {
            Ok((screen, on)) => {
                if *hub.config().kiosk.get(screen) == on {
                    continue;
                }
                let patch = serde_json::json!({ "kiosk": { name_of(screen): on } });
                if settings.send(patch).is_err() {
                    return;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

fn name_of(screen: Screen) -> &'static str {
    match screen {
        Screen::Main => "main",
        Screen::Cluster => "cluster",
        Screen::Aux => "aux",
    }
}

async fn follow_display_mode(mut applied: watch::Receiver<Config>, ctrl: CompositorControl) {
    let mut last = None;
    loop {
        let now = applied.borrow_and_update().display_mode.clone();
        if last.as_ref() != Some(&now) {
            ctrl.mode("main", &now.main);
            ctrl.mode("cluster", &now.cluster);
            ctrl.mode("aux", &now.aux);
            last = Some(now);
        }
        if applied.changed().await.is_err() {
            return;
        }
    }
}

async fn follow_audio_output(mut state: watch::Receiver<State>, gst: Output) {
    let mut device = state.borrow_and_update().config.audio_output_device.clone();
    while state.changed().await.is_ok() {
        let now = state.borrow_and_update().config.audio_output_device.clone();
        if now != device {
            gst.set_audio_output(now.as_deref().unwrap_or(""));
            device = now;
        }
    }
}

async fn follow_host(
    gst: Output,
    plane: Arc<MainPlane>,
    airplay: Arc<MainPlane>,
    clusters: Arc<ClusterPlanes>,
    state: watch::Receiver<State>,
) {
    let mut events = gst.subscribe();
    loop {
        match events.recv().await {
            Ok(OutputEvent::Config {
                plane: PLANE_MAIN,
                codec,
                atom,
            }) => {
                plane.prepare(codec, atom);
                place_carplay(&plane, &state.borrow().config);
            }
            Ok(OutputEvent::Config {
                plane: PLANE_AIRPLAY,
                codec,
                atom,
            }) => {
                airplay.prepare(codec, atom);
                airplay.set_crop(None);
            }
            Ok(OutputEvent::Config {
                plane: PLANE_CLUSTER_RECV,
                codec,
                atom,
            }) => {
                let cfg = state.borrow().config.clone();
                let size = (cfg.cluster_width, cfg.cluster_height);
                clusters.set_crop(crop_for(size, size));
                clusters.prepare(codec, atom);
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

fn carplay_tier(cfg: &Config) -> (u32, u32) {
    let or = |v: u32, d: u32| if v == 0 { d } else { v };
    (
        or(cfg.projection_width, 1920),
        or(cfg.projection_height, 1080),
    )
}

fn place_carplay(plane: &MainPlane, cfg: &Config) {
    let tier = carplay_tier(cfg);
    plane.set_crop(crop_for(
        tier,
        (cfg.projection_width, cfg.projection_height),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::config_file::defaults;

    #[tokio::test]
    async fn a_bluetooth_remote_reaches_the_phone_in_front() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = std::env::temp_dir().join(format!("livi-remote-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shared.sock");
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(remote_input(Shared::new(&path), tx));
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut asked = [0u8; 10];
        sock.read_exact(&mut asked).await.unwrap();
        assert_eq!(&asked, b"subscribe\n");
        sock.write_all(
            b"{\"event\":\"input\",\"command\":\"previous\"}\n\
              {\"event\":\"input\",\"command\":\"rewind\"}\n\
              {\"event\":\"input\",\"command\":\"eject\"}\n\
              {\"event\":\"input\",\"command\":\"playPause\"}\n",
        )
        .await
        .unwrap();
        assert_eq!(rx.recv().await, Some(UiCommand::Media(MediaControl::Prev)));
        assert_eq!(
            rx.recv().await,
            Some(UiCommand::Media(MediaControl::PlayPause))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn carplay_tier_falls_back_to_1080p() {
        let mut cfg = defaults();
        assert_eq!(carplay_tier(&cfg), (1280, 720));
        cfg.projection_width = 0;
        cfg.projection_height = 0;
        assert_eq!(carplay_tier(&cfg), (1920, 1080));
    }

    #[test]
    fn secondary_screens_follow_the_config() {
        let mut cfg = defaults();
        cfg.cluster_screen_active = false;
        cfg.aux_screen_active = false;
        cfg.cluster_screen_width = 800;
        cfg.cluster_screen_height = 480;
        cfg.aux_screen_width = 0;
        cfg.aux_screen_height = 0;
        assert_eq!(
            changed_screens(None, &cfg),
            vec![("cluster", false, Some((800, 480))), ("aux", false, None)],
            "the first state tells the compositor every role"
        );
        assert!(changed_screens(Some(&cfg), &cfg).is_empty());

        let mut on = cfg.clone();
        on.cluster_screen_active = true;
        assert_eq!(
            changed_screens(Some(&cfg), &on),
            vec![("cluster", true, Some((800, 480)))]
        );

        let mut resized = on.clone();
        resized.cluster_screen_width = 1024;
        assert_eq!(
            changed_screens(Some(&on), &resized),
            vec![("cluster", true, Some((1024, 480)))]
        );

        let mut off = resized.clone();
        off.cluster_screen_active = false;
        off.aux_screen_active = true;
        assert_eq!(
            changed_screens(Some(&resized), &off),
            vec![("cluster", false, Some((1024, 480))), ("aux", true, None)]
        );
    }

    #[test]
    fn the_host_lands_on_the_compositor_display() {
        let res = Resources {
            helper: Some(Helper::Process(PathBuf::from("/h"))),
            output: Some(OutputBackend::Process(PathBuf::from("/g"))),
            compositor: Some(PathBuf::from("/c")),
            gstreamer: None,
            templates: Some(PathBuf::from("/t")),
            ui: None,
            installer: None,
        };
        let ui = Ui {
            wayland_display: "wayland-1".into(),
            render_node: Some("/dev/dri/renderD128".into()),
        };
        let process = |launch| match launch {
            Some(Launch::Process(process)) => process,
            _ => panic!("the host should be a program"),
        };
        let launch = process(host_launch(&res, Path::new("/u"), Some(&ui)));
        let get = |k: &str| {
            launch
                .env
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("WAYLAND_DISPLAY"), Some("wayland-1"));
        assert_eq!(get("LIVI_RENDER_NODE"), Some("/dev/dri/renderD128"));
        let gl = cfg!(target_os = "linux").then_some("surfaceless");
        assert_eq!(
            get("GST_GL_WINDOW"),
            gl,
            "EGL without a window only on Linux"
        );
        assert_eq!(launch.log, PathBuf::from("/u/log/output.log"));
        assert_eq!(
            process(host_launch(&res, Path::new("/u"), None))
                .env
                .iter()
                .filter(|(k, _)| k == "WAYLAND_DISPLAY")
                .count(),
            0
        );
        assert!(host_launch(&Resources::embedded(), Path::new("/u"), None).is_none());
    }
}
