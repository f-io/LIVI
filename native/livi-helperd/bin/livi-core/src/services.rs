use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::config::Config;
use livi_core_proto::state::{Front, State};
use livi_cp::helper_sock::HelperSock;
use livi_cp::pairings::Pairings;
use livi_cp::stack::StackCtx;
use livi_host_proto::{PLANE_CLUSTER_RECV, PLANE_MAIN};
use tokio::net::TcpListener;
use tokio::process::Child;
use tokio::sync::{Notify, mpsc, oneshot, watch};

use crate::android_auto::{self, GstAaMedia};
use crate::carplay::{ConfigSource, GstMedia};
use crate::helper::{self, HelperSupervisor, Spec};
use crate::hub::Hub;
use crate::projection::{Drivers, Projection, UiAsks};
use crate::resources::{Resources, gst_env};
use crate::spectrum;
use crate::ui::UiSupervisor;
use livi_aa_stack::helper_sock::AaHelperSock;
use livi_media::compositor::{self, CompositorControl, Ui, backdrop_hex};
use livi_media::gst_host::{GstHost, HostEvent, Launch};
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
        let _lifeline = crate::child::hold_lifeline(&mut child);
        tokio::select! {
            status = child.wait() => {
                println!("[core] the compositor closed: {status:?}");
                quit.notify_one();
            }
            _ = stop_rx => {
                crate::child::terminate(&mut child, COMPOSITOR_STOP_GRACE, false).await;
            }
        }
    });
    Watched { stop, task }
}

/// Next to the AppImage, where the user looks for a crash report.
fn crash_log() -> PathBuf {
    let dir = std::env::var_os("APPIMAGE")
        .and_then(|a| Path::new(&a).parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    dir.join("livi-gst-host-crash.log")
}

fn host_launch(res: &Resources, user_data: &Path, ui: Option<&Ui>) -> Launch {
    let mut env = res.gstreamer.as_deref().map(gst_env).unwrap_or_default();
    if cfg!(target_os = "linux") {
        // For debugging a gst-host pipeline.
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
    Launch {
        bin: res.gst_host.clone(),
        env,
        log: user_data.join("log/gst-host.log"),
        crash: crash_log(),
    }
}

impl Services {
    pub fn start(
        res: Resources,
        runtime: &Path,
        user_data: &Path,
        hub: Arc<Hub>,
        core_socket: PathBuf,
        quit: Arc<Notify>,
        asks: UiAsks,
    ) -> Self {
        let state = hub.watch();
        let cfg = state.borrow().config.clone();
        let identity = livi_cp::identity::load_or_create(&user_data.join("cp/identity.json"));
        // The phone learns the port from the helper, so the system may pick any free one.
        let airplay = carplay_listener();
        let airplay_port = airplay.as_ref().map(|l| livi_cp::net::local_port(l.local_addr()));
        let helper_restart = Arc::new(Notify::new());
        let applied = hub.applied();
        let env_identity = identity.clone();
        let helper = HelperSupervisor::start(Spec {
            bin: helper::resolve_bin(&res.helper, user_data),
            env: Box::new(move || {
                let cfg = applied.borrow().clone();
                helper::helper_env(&cfg, &env_identity, airplay_port)
            }),
            restart: helper_restart.clone(),
            use_sudo: cfg!(target_os = "linux"),
            restart_delay: HELPER_RESTART_DELAY,
            max_restarts: Some(HELPER_MAX_RESTARTS),
        });

        let ctrl_path = runtime.join("compositor.ctrl");
        // macOS draws the video into the UI's window, there is no compositor.
        let compositor = if cfg!(target_os = "linux") {
            let started = compositor::start(
                &res.compositor,
                &cfg,
                &ctrl_path,
                &user_data.join("log/compositor.log"),
            );
            if started.is_none() {
                println!("[core] running without the compositor");
            }
            started
        } else {
            None
        };
        let ctrl = CompositorControl::connect(ctrl_path);
        let gst = GstHost::new(runtime.join("gst.sock"));
        gst.set_launch(host_launch(&res, user_data, None));
        let plane = MainPlane::new(gst.clone(), ctrl.clone());
        let clusters = ClusterPlanes::new(gst.clone(), ctrl.clone());

        let display = compositor.is_some().then(|| ctrl.ui());
        let ui =
            res.ui.clone().map(|cmd| UiSupervisor::start(cmd, core_socket, display, quit.clone()));
        if ui.is_none() {
            println!("[core] running without a UI");
        }
        let compositor = compositor.map(|child| watch_compositor(child, quit));
        tokio::spawn(spectrum::follow(gst.clone(), asks.spectrum.clone(), state.clone()));
        tokio::spawn(crate::blinker::run(hub.clone(), gst.clone()));

        if let Some(listener) = airplay {
            let carplay = CarPlay {
                listener,
                identity,
                user_data: user_data.to_path_buf(),
                probe: host_launch(&res, user_data, None),
                debug: helper::debug(&cfg),
                helper_restart,
            };
            carplay.start(hub, &ctrl, &gst, plane.clone(), clusters.clone(), asks);
        }

        tokio::spawn(follow_display(ctrl.ui(), gst.clone(), res, user_data.to_path_buf()));
        tokio::spawn(follow_state(state.clone(), ctrl, plane.clone(), clusters.clone()));
        tokio::spawn(follow_audio_output(state.clone(), gst.clone()));
        tokio::spawn(follow_host(gst, plane, clusters, state));

        Self { helper: Some(helper), compositor, ui }
    }

    /// The UI first, so it does not see its display vanish and count a crash.
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
    listener: TcpListener,
    identity: livi_cp::identity::Identity,
    user_data: PathBuf,
    probe: Launch,
    debug: bool,
    helper_restart: Arc<Notify>,
}

impl CarPlay {
    fn start(
        self,
        hub: Arc<Hub>,
        ctrl: &CompositorControl,
        gst: &GstHost,
        plane: Arc<MainPlane>,
        clusters: Arc<ClusterPlanes>,
        asks: UiAsks,
    ) {
        let state = hub.watch();
        let (source, config, aa_config) = ConfigSource::new(state.clone(), ctrl.panels());
        source.follow();
        let probing = source.clone();
        let probe = self.probe;
        tokio::spawn(async move {
            let Some(codecs) = livi_media::gst_host::probe_codecs(&probe).await else { return };
            println!("[core] gst-host codecs {codecs}");
            probing.set_codecs(android_auto::codecs(&codecs));
            let _ = tokio::task::spawn_blocking(move || probing.refresh()).await;
        });
        let refreshing = source.clone();
        let ctx = Arc::new(StackCtx {
            identity: self.identity,
            pairings: Pairings::new(self.user_data.join("cp/pairings.json")),
            helper: HelperSock::default(),
            media: GstMedia::new(gst.clone(), VideoPlane::new(gst.clone(), ctrl.clone())),
            config,
            refresh: Some(Arc::new(move || refreshing.refresh())),
            debug: self.debug,
            // gst-host plays URLs on Linux only so far.
            video_playback: cfg!(not(target_os = "macos")),
            video_capture: self.debug.then(|| self.user_data.join("log/video-playback")),
        });
        let (events_tx, events) = mpsc::unbounded_channel();
        let cp = livi_cp::manager::start(self.listener, ctx, events_tx);
        let wants = *asks.wants.borrow();
        let devices = crate::devices::Devices::new(
            &self.user_data.join("devices.json"),
            HelperSock::default(),
        );
        let aa_media = GstAaMedia::new(gst.clone(), plane.clone(), clusters.clone(), state.clone());
        let (aa_events_tx, aa_events) = mpsc::unbounded_channel();
        let aa = livi_aa_stack::manager::start(
            AaHelperSock::default(),
            aa_media.clone(),
            aa_config,
            aa_events_tx,
        );
        let (phone_io, phone_asks) = mpsc::unbounded_channel();
        let (phone_answers, phone_back) = mpsc::unbounded_channel();
        tokio::spawn(crate::android_auto::hands_free_io(
            phone_asks,
            aa_media.clone(),
            AaHelperSock::default(),
            phone_answers,
        ));
        let drivers = Drivers { cp, aa, aa_crop: aa_media.crop(), phone_io, phone_back };
        let projection =
            Projection::new(hub, drivers, plane, clusters, self.helper_restart, devices, wants);
        tokio::spawn(projection.run(events, aa_events, asks, state));
    }
}

/// gst-host has to land on the compositor's display and decode on its GPU.
async fn follow_display(
    mut ui: watch::Receiver<Option<Ui>>,
    gst: GstHost,
    res: Resources,
    user_data: PathBuf,
) {
    while ui.changed().await.is_ok() {
        let current = ui.borrow_and_update().clone();
        if let Some(ui) = current {
            println!(
                "[core] compositor display {} on {}",
                ui.wayland_display,
                ui.render_node.as_deref().unwrap_or("-")
            );
            gst.set_launch(host_launch(&res, &user_data, Some(&ui)));
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

async fn follow_state(
    mut state: watch::Receiver<State>,
    ctrl: CompositorControl,
    plane: Arc<MainPlane>,
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
        if last.as_ref().is_none_or(|l| l.front.main != now.front.main) {
            plane.set_visible(now.front.main == Front::Projection);
        }
        if last.as_ref().is_none_or(|l| l.front != now.front) {
            let f = &now.front;
            clusters.show_on([f.main, f.dash, f.aux].map(|f| f == Front::Cluster));
        }
        last = Some(now);
        if state.changed().await.is_err() {
            return;
        }
    }
}

async fn follow_audio_output(mut state: watch::Receiver<State>, gst: GstHost) {
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
    gst: GstHost,
    plane: Arc<MainPlane>,
    clusters: Arc<ClusterPlanes>,
    state: watch::Receiver<State>,
) {
    let mut events = gst.subscribe();
    loop {
        match events.recv().await {
            Ok(HostEvent::Config { plane: PLANE_MAIN, codec, atom }) => {
                plane.prepare(codec, atom);
                place_carplay(&plane, &state.borrow().config);
            }
            Ok(HostEvent::Config { plane: PLANE_CLUSTER_RECV, codec, atom }) => {
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
    (or(cfg.projection_width, 1920), or(cfg.projection_height, 1080))
}

fn place_carplay(plane: &MainPlane, cfg: &Config) {
    let tier = carplay_tier(cfg);
    plane.set_crop(crop_for(tier, (cfg.projection_width, cfg.projection_height)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::defaults;

    #[test]
    fn carplay_tier_falls_back_to_1080p() {
        let mut cfg = defaults();
        assert_eq!(carplay_tier(&cfg), (1280, 720));
        cfg.projection_width = 0;
        cfg.projection_height = 0;
        assert_eq!(carplay_tier(&cfg), (1920, 1080));
    }

    #[test]
    fn the_host_lands_on_the_compositor_display() {
        let res = Resources {
            helper: PathBuf::from("/h"),
            gst_host: PathBuf::from("/g"),
            compositor: PathBuf::from("/c"),
            gstreamer: None,
            templates: PathBuf::from("/t"),
            ui: None,
        };
        let ui = Ui {
            wayland_display: "wayland-1".into(),
            render_node: Some("/dev/dri/renderD128".into()),
        };
        let launch = host_launch(&res, Path::new("/u"), Some(&ui));
        let get = |k: &str| launch.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("WAYLAND_DISPLAY"), Some("wayland-1"));
        assert_eq!(get("LIVI_RENDER_NODE"), Some("/dev/dri/renderD128"));
        let gl = cfg!(target_os = "linux").then_some("surfaceless");
        assert_eq!(get("GST_GL_WINDOW"), gl, "EGL without a window only on Linux");
        assert_eq!(launch.log, PathBuf::from("/u/log/gst-host.log"));
        assert_eq!(
            host_launch(&res, Path::new("/u"), None)
                .env
                .iter()
                .filter(|(k, _)| k == "WAYLAND_DISPLAY")
                .count(),
            0
        );
    }
}
