// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use livi_aa_stack::config::{AaConfig, Addresses as AaAddresses, Codecs, ScreenWidths};
use livi_core_proto::config::{CarType, ClusterContent, Config, DockPosition, HandDriveType};
use livi_core_proto::icon::{
    DEFAULT_120 as ICON_120, DEFAULT_180 as ICON_180, DEFAULT_256 as ICON_256,
};
use livi_core_proto::state::{Screen, State};
use livi_core_proto::theme::{DisplayKind, Theme};
use livi_cp::info::{DisplayConfig, Icon, InfoConfig, Insets, ThemeDisplays, mm_at_dpi};
use livi_cp::media::{AudioCodec, AudioRequest, AudioStream, Media, MicRequest, VideoStatus};
use livi_cp::stack::StackConfig;
use livi_cp::vehicle_state::{Engines, Rpm};
use livi_media::compositor::Panel;
use livi_media::output::{self, Output, OutputEvent};
use livi_media::planes::VideoPlane;
use livi_output_proto::{
    PLANE_CLUSTER_RECV, PLANE_MAIN, PLANE_VIDEO, Probe, UrlStatus, VideoCodec,
};
use tokio::sync::{broadcast, watch};

use crate::{system::host, system::hwaddr};

const LOGO: &[u8] = include_bytes!("../../res/livi-logo.png");

pub struct GstMedia {
    gst: Output,
    video: Arc<VideoPlane>,
    started: broadcast::Sender<(u32, u32)>,
    video_status: Arc<Mutex<Option<VideoStatus>>>,
    output_delays: Arc<Mutex<HashMap<u32, u32>>>,
}

impl GstMedia {
    pub fn new(gst: Output, video: Arc<VideoPlane>) -> Arc<Self> {
        let (started, _) = broadcast::channel(32);
        let mut events = gst.subscribe();
        let tx = started.clone();
        let video_status = Arc::new(Mutex::new(None));
        let latest = video_status.clone();
        let output_delays = Arc::new(Mutex::new(HashMap::new()));
        let delays = output_delays.clone();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(OutputEvent::AudioStarted {
                        stream,
                        first_sample,
                    }) => {
                        let _ = tx.send((stream, first_sample));
                    }
                    Ok(OutputEvent::AudioDelay { stream, ms }) => {
                        delays
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(stream, ms);
                    }
                    Ok(OutputEvent::UrlStatus {
                        plane: PLANE_VIDEO,
                        status,
                    }) => {
                        *latest.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(video_status_of(status));
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Arc::new(Self {
            gst,
            video,
            started,
            video_status,
            output_delays,
        })
    }
}

impl Media for GstMedia {
    fn open_screen(&self, cluster: bool, key: [u8; 32]) -> impl Future<Output = (u16, u32)> + Send {
        let plane = if cluster {
            PLANE_CLUSTER_RECV
        } else {
            PLANE_MAIN
        };
        async move { self.gst.open_video_receiver(plane, &key, cluster).await }
    }

    fn set_screen_active(&self, receiver: u32, active: bool) {
        self.gst.set_active_feeder(receiver, active);
    }

    fn close_screen(&self, receiver: u32) {
        self.gst.close_video_receiver(receiver);
    }

    fn open_audio(
        &self,
        key: [u8; 32],
        req: AudioRequest,
    ) -> impl Future<Output = AudioStream> + Send {
        let opts = output::AudioOpts {
            codec: match req.codec {
                AudioCodec::AacLc => output::AudioCodec::AacLc,
                AudioCodec::Opus => output::AudioCodec::Opus,
                AudioCodec::Pcm => output::AudioCodec::Pcm,
            },
            payload_type: req.payload_type,
            clock_rate: req.clock_rate,
            channels: req.channels,
            latency_ms: req.latency_ms,
            realtime: req.realtime,
            fed: false,
            device: req.device,
        };
        async move {
            let (id, data_port, control_port) = self.gst.open_audio(&key, &opts).await;
            AudioStream {
                id,
                data_port,
                control_port,
            }
        }
    }

    fn set_audio_active(&self, stream: u32, active: bool) {
        self.gst.set_audio_active(stream, active);
    }

    fn set_audio_volume(&self, stream: u32, level: f64, ramp_ms: u32) {
        self.gst.set_audio_volume(stream, level, ramp_ms);
    }

    fn audio_output_delay(&self, stream: u32) -> Option<u32> {
        self.output_delays
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&stream)
            .copied()
    }

    fn close_audio(&self, stream: u32) {
        self.gst.close_audio(stream);
    }

    fn open_mic(&self, key: [u8; 32], req: MicRequest) -> u32 {
        self.gst.open_mic(
            &key,
            &output::MicOpts {
                pcm: !req.opus,
                payload_type: req.payload_type,
                sample_rate: req.sample_rate,
                channels: req.channels,
                bitrate: req.bitrate,
                frame_ms: req.frame_ms,
                port: req.port,
                phone: req.phone,
                device: req.device,
            },
        )
    }

    fn close_mic(&self, id: u32) {
        self.gst.close_mic(id);
    }

    fn audio_started(&self) -> broadcast::Receiver<(u32, u32)> {
        self.started.subscribe()
    }

    fn play_video(&self, url: String, audio_device: String, muted: bool) {
        self.forget_video_status();
        let plane = self.video.clone();
        let turn = plane.turn();
        tokio::spawn(async move {
            if plane.play(turn, &url, &audio_device).await {
                if muted {
                    plane.set_muted(true);
                }
                println!("[core] video {url} on its own plane");
            }
        });
    }

    fn set_video_rate(&self, rate: f64) {
        self.video.set_rate(rate);
    }

    fn seek_video(&self, seconds: f64) {
        self.video.seek(seconds);
    }

    fn set_video_muted(&self, muted: bool) {
        self.video.set_muted(muted);
    }

    fn set_video_shown(&self, shown: bool) {
        self.video.set_shown(shown);
    }

    fn stop_video(&self) {
        self.video.stop();
        self.forget_video_status();
        println!("[core] video plane closed");
    }

    fn video_status(&self) -> Option<VideoStatus> {
        *self.video_status.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl GstMedia {
    fn forget_video_status(&self) {
        *self.video_status.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn video_status_of(s: UrlStatus) -> VideoStatus {
    VideoStatus {
        position: s.position,
        duration: s.duration,
        seekable: s.seekable,
        playing: s.playing,
        ready: s.ready,
        buffered_percent: s.buffered_percent,
        ended: s.ended,
        failed: s.failed,
    }
}

fn even(v: u32) -> u32 {
    v - v % 2
}

pub fn cluster_displayed(cfg: &Config) -> bool {
    let d = cfg.dashboard;
    d.main || d.cluster || d.aux
}

fn status_bar_edge(dock: DockPosition) -> u64 {
    match dock {
        DockPosition::Auto => 0,
        DockPosition::Bottom => 1,
        DockPosition::Side => 2,
    }
}

fn panel_mm(panel: Option<&Panel>, (w, h): (u32, u32)) -> Option<(i64, i64)> {
    let s = panel?;
    if s.width_mm == 0 || s.height_mm == 0 {
        return None;
    }
    let turn = (w > h && s.width_mm < s.height_mm) || (w < h && s.width_mm > s.height_mm);
    let (mm_w, mm_h, px_w, px_h) = if turn {
        (s.height_mm, s.width_mm, s.height_px, s.width_px)
    } else {
        (s.width_mm, s.height_mm, s.width_px, s.height_px)
    };
    if px_w == 0 || px_h == 0 || w == 0 || h == 0 {
        return Some((mm_w.into(), mm_h.into()));
    }
    let fit = (f64::from(px_w) / f64::from(w)).min(f64::from(px_h) / f64::from(h));
    let shown = |px: u32, mm: u32, panel_px: u32| {
        (f64::from(px) * fit * f64::from(mm) / f64::from(panel_px)).round() as i64
    };
    Some((shown(w, mm_w, px_w), shown(h, mm_h, px_h)))
}

fn measured_mm(cfg: &Config, screen: Screen) -> Option<(i64, i64)> {
    let (w, h) = (
        *cfg.screen_width_mm.get(screen),
        *cfg.screen_height_mm.get(screen),
    );
    (w > 0 && h > 0).then(|| (w.into(), h.into()))
}

fn oriented((w_mm, h_mm): (i64, i64), (w, h): (u32, u32)) -> (i64, i64) {
    if (w > h && w_mm < h_mm) || (w < h && w_mm > h_mm) {
        (h_mm, w_mm)
    } else {
        (w_mm, h_mm)
    }
}

fn main_size(cfg: &Config) -> (u32, u32) {
    let or = |v: u32, d: u32| if v == 0 { d } else { v };
    (
        even(or(cfg.projection_width, 1920)),
        even(or(cfg.projection_height, 1080)),
    )
}

fn main_mm(
    cfg: &Config,
    panels: &HashMap<String, Panel>,
    screen: Option<&Panel>,
) -> Option<(i64, i64)> {
    let size = main_size(cfg);
    measured_mm(cfg, Screen::Main)
        .map(|mm| oriented(mm, size))
        .or_else(|| panel_mm(panels.get("main"), size))
        .or_else(|| panel_mm(screen, size))
}

fn cluster_mm(cfg: &Config, panels: &HashMap<String, Panel>) -> Option<(i64, i64)> {
    let size = (cfg.cluster_width, cfg.cluster_height);
    measured_mm(cfg, Screen::Cluster)
        .map(|mm| oriented(mm, size))
        .or_else(|| panel_mm(panels.get("cluster"), size))
}

fn dpi_mm(dpi: u32, w: u32, h: u32) -> Option<(i64, i64)> {
    (dpi > 0).then(|| {
        let mm = |px: u32| mm_at_dpi(px.into(), dpi.into());
        (mm(w), mm(h))
    })
}

fn insets(top: u32, bottom: u32, left: u32, right: u32) -> Option<Insets> {
    Some(Insets {
        top: top.into(),
        bottom: bottom.into(),
        left: left.into(),
        right: right.into(),
    })
}

fn icons(cfg: &Config) -> Vec<Icon> {
    let pick = |own: &Option<String>, default: &str| {
        own.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(default)
            .trim()
            .to_string()
    };
    [
        (120, pick(&cfg.carplay_icon120, ICON_120)),
        (180, pick(&cfg.carplay_icon180, ICON_180)),
        (256, pick(&cfg.carplay_icon256, ICON_256)),
    ]
    .into_iter()
    .filter_map(|(size, b64)| {
        let png = STANDARD.decode(b64).ok().filter(|d| !d.is_empty())?;
        Some(Icon {
            width_pixels: size,
            height_pixels: size,
            png,
        })
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addresses {
    pub accessory: String,
    pub wifi_bssid: String,
    pub bt_mac: String,
    pub access_point: Option<livi_wifi::OnAir>,
}

pub fn stack_config(
    cfg: &Config,
    panels: &HashMap<String, Panel>,
    screen: Option<Panel>,
    hevc: bool,
    addresses: &Addresses,
    theme: &Theme,
) -> StackConfig {
    let or = |v: u32, d: u32| if v == 0 { d } else { v };
    let [main_ui, main_map, cluster_ui, cluster_map] = cfg.night_modes();
    let (main_w, main_h) = main_size(cfg);
    let main_mm = dpi_mm(cfg.projection_dpi, main_w, main_h)
        .or_else(|| main_mm(cfg, panels, screen.as_ref()));
    let name = if cfg.car_name.trim().is_empty() {
        "LIVI".to_string()
    } else {
        cfg.car_name.clone()
    };
    let cluster = cluster_displayed(cfg).then(|| {
        let mm = dpi_mm(cfg.cluster_dpi, cfg.cluster_width, cfg.cluster_height)
            .or_else(|| cluster_mm(cfg, panels));
        DisplayConfig {
            width_pixels: cfg.cluster_width.into(),
            height_pixels: cfg.cluster_height.into(),
            width_physical_mm: mm.map(|m| m.0),
            height_physical_mm: mm.map(|m| m.1),
            fps: Some(or(cfg.cluster_fps, 60).into()),
            view_area: insets(
                cfg.cluster_view_area_top,
                cfg.cluster_view_area_bottom,
                cfg.cluster_view_area_left,
                cfg.cluster_view_area_right,
            ),
            safe_area: insets(
                cfg.cluster_safe_area_top,
                cfg.cluster_safe_area_bottom,
                cfg.cluster_safe_area_left,
                cfg.cluster_safe_area_right,
            ),
            ui_night: cluster_ui,
            map_night: cluster_map,
            ..Default::default()
        }
    });
    let source = cfg.car_play_source_version.trim();
    StackConfig {
        info: InfoConfig {
            device_name: name.clone(),
            oem_label: if cfg.oem_name.trim().is_empty() {
                name
            } else {
                cfg.oem_name.clone()
            },
            icons: icons(cfg),
            right_hand_drive: cfg.hand == HandDriveType::Rhd,
            theme_displays: theme_displays(cfg, theme),
            appearance_setting: cfg.appearance_setting.code(),
            device_id: addresses.accessory.clone(),
            bt_mac: addresses.bt_mac.clone(),
            source_version: if source.is_empty() {
                livi_core_proto::config::defaults().car_play_source_version
            } else {
                source.to_string()
            },
            hevc,
            main: DisplayConfig {
                width_pixels: main_w.into(),
                height_pixels: main_h.into(),
                width_physical_mm: main_mm.map(|m| m.0),
                height_physical_mm: main_mm.map(|m| m.1),
                fps: Some(or(cfg.projection_fps, 60).into()),
                primary_input_device: Some(1),
                view_area: insets(
                    cfg.projection_view_area_top,
                    cfg.projection_view_area_bottom,
                    cfg.projection_view_area_left,
                    cfg.projection_view_area_right,
                ),
                safe_area: insets(
                    cfg.projection_safe_area_top,
                    cfg.projection_safe_area_bottom,
                    cfg.projection_safe_area_left,
                    cfg.projection_safe_area_right,
                ),
                safe_area_draw_outside: Some(cfg.projection_safe_area_draw_outside),
                status_bar_edge: Some(status_bar_edge(cfg.car_play_dock_position)),
                initial_url: None,
                ui_night: main_ui,
                map_night: main_map,
            },
            cluster,
            entertainment_sample_rate: if cfg.sampling_frequency == 1 {
                48000
            } else {
                44100
            },
            disable_audio_output: cfg.disable_audio_output,
        },
        audio_device: cfg.audio_output_device.clone().unwrap_or_default(),
        audio_input_device: cfg.audio_input_device.clone().unwrap_or_default(),
        vehicle_climate: cfg.car_play_vehicle_climate,
        app_version: env!("CARGO_PKG_VERSION").into(),
        app_build_date: option_env!("LIVI_BUILD_DATE").unwrap_or_default().into(),
        cluster_url: cluster_url(cfg),
        vehicle_variant: cfg.car_play_vehicle_model.clone(),
        engines: engines(cfg),
        cluster_content: theme_displays(cfg, theme).map(|names| {
            livi_cp::vehicle_state::ClusterContent {
                panel: names.cluster,
                zones: [String::new(), String::new()],
            }
        }),
        images: Vec::new(),
    }
}

fn theme_displays(cfg: &Config, cluster: &Theme) -> Option<ThemeDisplays> {
    if cfg.car_play_theme.is_empty() {
        return None;
    }
    let id = |kind| {
        cluster
            .displays
            .iter()
            .find(|d| d.kind == kind)
            .map(|d| d.id.clone())
    };
    Some(ThemeDisplays {
        main: id(DisplayKind::Center)?,
        cluster: id(DisplayKind::Cluster)?,
    })
}

fn engines(cfg: &Config) -> Engines {
    use CarType::*;
    let combustion = matches!(
        cfg.car_type,
        Some(
            Gasoline
                | DieselWinter
                | Diesel
                | Biodiesel
                | E85
                | Lpg
                | Cng
                | Lng
                | HybridGasoline
                | HybridDiesel
        )
    );
    let electric = matches!(
        cfg.car_type,
        Some(Electric | Hydrogen | HybridGasoline | HybridDiesel)
    );
    Engines {
        combustion: combustion.then(|| Rpm {
            max: cfg.max_rpm,
            redline: cfg.redline_rpm.min(cfg.max_rpm),
        }),
        electric,
    }
}

fn cluster_url(cfg: &Config) -> String {
    let path = match cfg.car_play_cluster_content {
        ClusterContent::Auto => "maps:/car/instrumentcluster",
        ClusterContent::Map => "maps:/car/instrumentcluster/map",
        ClusterContent::Maneuver => return "maps:/car/instrumentcluster/instructioncard".into(),
    };
    let user = |on: bool| if on { "user" } else { "no" };
    format!(
        "{path}?showCompass={}&showETA={}&showSpeedLimit={}",
        user(cfg.car_play_cluster_compass),
        if cfg.car_play_cluster_eta {
            "yes"
        } else {
            "no"
        },
        user(cfg.car_play_cluster_speed_limit)
    )
}

pub fn offers_hevc(probe: &Probe) -> bool {
    let (h264, h265) = (probe.get(VideoCodec::H264), probe.get(VideoCodec::H265));
    h265.hw || (h265.sw && !h264.hw)
}

fn aa_config(
    cfg: &Config,
    panels: &HashMap<String, Panel>,
    screen: Option<&Panel>,
    codecs: Codecs,
    addresses: &Addresses,
) -> AaConfig {
    let widths = ScreenWidths {
        main: main_mm(cfg, panels, screen).map(|m| m.0 as f64),
        cluster: cluster_mm(cfg, panels).map(|m| m.0 as f64),
    };
    let known = |a: &str| (a != hwaddr::FALLBACK).then(|| a.to_string());
    let addresses = AaAddresses {
        bt_mac: known(&addresses.bt_mac),
        wifi_bssid: known(&addresses.wifi_bssid),
        access_point: addresses.access_point.clone(),
    };
    livi_aa_stack::config::from_livi(
        cfg,
        widths,
        codecs,
        cfg.appearance_mode.night_mode(),
        &addresses,
    )
}

pub struct ConfigSource {
    tx: watch::Sender<StackConfig>,
    aa: watch::Sender<AaConfig>,
    state: watch::Receiver<State>,
    panels: watch::Receiver<HashMap<String, Panel>>,
    screen: Option<Panel>,
    codecs: Mutex<Codecs>,
    addresses: Mutex<Addresses>,
    told: Mutex<HashMap<String, String>>,
    accessory: String,
    images: PathBuf,
}

fn carplay_images(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let logo = dir.join("livi.png");
    if !logo.exists() {
        let written = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&logo, LOGO));
        if let Err(e) = written {
            println!("[core] cannot write {}: {e}", logo.display());
        }
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut images: Vec<_> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let png = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"));
            let name = path.file_stem()?.to_str()?.to_string();
            png.then(|| Some((name, std::fs::read(&path).ok()?)))
                .flatten()
        })
        .collect();
    images.sort();
    images
}

fn with_images(mut cfg: StackConfig, images: &Path) -> StackConfig {
    cfg.images = carplay_images(images);
    cfg
}

fn remembered(told: &mut HashMap<String, String>, key: String, found: Option<String>) -> String {
    match found {
        Some(address) => {
            told.insert(key, address.clone());
            address
        }
        None => told
            .get(&key)
            .cloned()
            .unwrap_or_else(|| hwaddr::FALLBACK.into()),
    }
}

impl ConfigSource {
    pub fn new(
        state: watch::Receiver<State>,
        panels: watch::Receiver<HashMap<String, Panel>>,
        accessory: String,
        images: PathBuf,
    ) -> (
        Arc<Self>,
        watch::Receiver<StackConfig>,
        watch::Receiver<AaConfig>,
    ) {
        println!("[core] CarPlay accessory {accessory}");
        let addresses = Addresses {
            accessory: accessory.clone(),
            wifi_bssid: hwaddr::FALLBACK.into(),
            bt_mac: if cfg!(target_os = "android") {
                hwaddr::android_bt_mac().unwrap_or_else(|| accessory.clone())
            } else {
                hwaddr::FALLBACK.into()
            },
            access_point: None,
        };
        let (cfg, cluster) = {
            let s = state.borrow();
            (s.config.clone(), s.cluster.theme.clone())
        };
        let screen = host::screen_panel();
        let first = with_images(
            stack_config(&cfg, &panels.borrow(), screen, false, &addresses, &cluster),
            &images,
        );
        let (tx, rx) = watch::channel(first);
        let (aa, aa_rx) = watch::channel(aa_config(
            &cfg,
            &panels.borrow(),
            screen.as_ref(),
            Codecs::default(),
            &addresses,
        ));
        let source = Arc::new(Self {
            tx,
            aa,
            state,
            panels,
            screen,
            codecs: Mutex::new(Codecs::default()),
            addresses: Mutex::new(addresses),
            told: Mutex::new(HashMap::new()),
            accessory,
            images,
        });
        (source, rx, aa_rx)
    }

    pub fn refresh(&self) {
        let (cfg, cluster, dongle_up) = {
            let s = self.state.borrow();
            (
                s.config.clone(),
                s.cluster.theme.clone(),
                s.system.dongle.is_some(),
            )
        };
        let reachable = |name: &str| dongle_up || name != livi_link_host::link::CHOICE;
        let wifi = reachable(&cfg.wifi_interface);
        let found = {
            let mut told = self.told.lock().unwrap_or_else(|e| e.into_inner());
            Addresses {
                accessory: self.accessory.clone(),
                wifi_bssid: remembered(
                    &mut told,
                    format!("wifi {}", cfg.wifi_interface),
                    wifi.then(|| hwaddr::accessory_id(&cfg.wifi_interface))
                        .flatten(),
                ),
                bt_mac: if cfg!(target_os = "android")
                    && cfg.bt_adapter != livi_link_host::link::CHOICE
                {
                    hwaddr::android_bt_mac().unwrap_or_else(|| self.accessory.clone())
                } else {
                    remembered(
                        &mut told,
                        format!("bt {}", cfg.bt_adapter),
                        reachable(&cfg.bt_adapter)
                            .then(|| hwaddr::bt_mac(&cfg.bt_adapter))
                            .flatten(),
                    )
                },
                access_point: wifi
                    .then(|| livi_link_host::on_air(&cfg.wifi_interface))
                    .flatten(),
            }
        };
        let addresses = {
            let mut known = self.addresses.lock().unwrap_or_else(|e| e.into_inner());
            if *known != found {
                if (&known.wifi_bssid, &known.bt_mac) != (&found.wifi_bssid, &found.bt_mac) {
                    println!(
                        "[core] Wi-Fi {} and bt {} for Android Auto",
                        found.wifi_bssid, found.bt_mac
                    );
                }
                *known = found;
            }
            known.clone()
        };
        let codecs = *self.codecs.lock().unwrap_or_else(|e| e.into_inner());
        let next = with_images(
            stack_config(
                &cfg,
                &self.panels.borrow(),
                self.screen,
                codecs.hevc,
                &addresses,
                &cluster,
            ),
            &self.images,
        );
        self.tx.send_if_modified(|cur| {
            let changed = *cur != next;
            *cur = next;
            changed
        });
        livi_aa_stack::log::set_debug(cfg.debug_logging);
        let aa = aa_config(
            &cfg,
            &self.panels.borrow(),
            self.screen.as_ref(),
            codecs,
            &addresses,
        );
        self.aa.send_if_modified(|cur| {
            let changed = *cur != aa;
            *cur = aa;
            changed
        });
    }

    pub fn set_codecs(&self, codecs: Codecs) {
        println!(
            "[core] codecs hevc {} vp9 {} av1 {}",
            codecs.hevc, codecs.vp9, codecs.av1
        );
        *self.codecs.lock().unwrap_or_else(|e| e.into_inner()) = codecs;
    }

    pub fn follow(self: &Arc<Self>) {
        let source = self.clone();
        let mut state = self.state.clone();
        let mut panels = self.panels.clone();
        tokio::spawn(async move {
            let mut seen = None;
            let mut panels_moved = true;
            loop {
                let now = {
                    let s = state.borrow_and_update();
                    let sys = &s.system;
                    (
                        s.config.clone(),
                        sys.dongle,
                        sys.wifi_interfaces.clone(),
                        sys.bt_adapters.clone(),
                        s.cluster.theme.id.clone(),
                    )
                };
                if panels_moved || seen.as_ref() != Some(&now) {
                    let refresh = source.clone();
                    let _ = tokio::task::spawn_blocking(move || refresh.refresh()).await;
                    seen = Some(now);
                    panels_moved = false;
                }
                tokio::select! {
                    changed = state.changed() => if changed.is_err() { return },
                    changed = panels.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        panels_moved = true;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use livi_core_proto::config::defaults;
    use livi_core_proto::theme::livi_theme;

    use super::*;

    fn panels() -> HashMap<String, Panel> {
        HashMap::from([(
            "main".to_string(),
            Panel {
                width_mm: 300,
                height_mm: 170,
                width_px: 1280,
                height_px: 720,
            },
        )])
    }

    fn addresses() -> Addresses {
        Addresses {
            accessory: "11:11:11:11:11:11".into(),
            wifi_bssid: "33:33:33:33:33:33".into(),
            bt_mac: "22:22:22:22:22:22".into(),
            access_point: None,
        }
    }

    #[test]
    fn the_config_carries_screen_names_and_addresses() {
        let mut cfg = defaults();
        cfg.projection_width = 1281;
        cfg.projection_height = 721;
        cfg.car_name = "  ".into();
        cfg.oem_name = "Mine".into();
        cfg.hand = HandDriveType::Rhd;
        cfg.sampling_frequency = 1;
        cfg.audio_output_device = Some("sink".into());
        cfg.dashboard.main = false;
        let sc = stack_config(&cfg, &panels(), None, true, &addresses(), &livi_theme());
        assert_eq!(sc.info.main.width_pixels, 1280);
        assert_eq!(sc.info.main.height_pixels, 720);
        assert_eq!(sc.info.main.width_physical_mm, Some(300));
        assert_eq!(sc.info.device_name, "LIVI");
        assert_eq!(sc.info.oem_label, "Mine");
        assert!(sc.info.right_hand_drive);
        assert!(sc.info.hevc);
        assert_eq!(sc.info.entertainment_sample_rate, 48000);
        assert_eq!(sc.info.device_id, "11:11:11:11:11:11");
        assert_eq!(sc.info.icons.len(), 3);
        assert_eq!(sc.audio_device, "sink");
        assert!(sc.info.cluster.is_none());
    }

    #[test]
    fn a_missed_answer_keeps_the_address_last_told_there() {
        let mut told = HashMap::new();
        assert_eq!(
            remembered(&mut told, "wifi livi-link".into(), None),
            hwaddr::FALLBACK
        );
        let mac = Some("88:00:33:77:8C:26".to_string());
        assert_eq!(
            remembered(&mut told, "wifi livi-link".into(), mac),
            "88:00:33:77:8C:26"
        );
        assert_eq!(
            remembered(&mut told, "wifi livi-link".into(), None),
            "88:00:33:77:8C:26"
        );
        assert_eq!(
            remembered(&mut told, "wifi wlan0".into(), None),
            hwaddr::FALLBACK
        );
    }

    #[test]
    fn an_absent_dongle_is_not_asked_and_its_address_stays() {
        use livi_core_proto::state::{Front, PerScreen, State};
        let mut config = defaults();
        config.wifi_interface = livi_link_host::link::CHOICE.into();
        config.bt_adapter = livi_link_host::link::CHOICE.into();
        let state = State {
            front: PerScreen {
                main: Front::Livi,
                cluster: Front::Livi,
                aux: Front::Livi,
            },
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
            config,
        };
        let (_state_tx, state_rx) = watch::channel(state);
        let (_panels_tx, panels_rx) = watch::channel(panels());
        let images =
            std::env::temp_dir().join(format!("livi-source-images-{}", std::process::id()));
        let (source, config, aa) = ConfigSource::new(
            state_rx,
            panels_rx,
            "02:5A:11:22:33:44".into(),
            images.clone(),
        );
        source
            .told
            .lock()
            .unwrap()
            .insert("wifi livi-link".into(), "88:00:33:77:8C:26".into());
        let started = std::time::Instant::now();
        source.refresh();
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(config.borrow().info.device_id, "02:5A:11:22:33:44");
        assert_eq!(aa.borrow().wifi_bssid.as_deref(), Some("88:00:33:77:8C:26"));
        let _ = std::fs::remove_dir_all(&images);
    }

    #[test]
    fn a_cluster_dashboard_brings_the_second_screen() {
        let mut cfg = defaults();
        cfg.projection_width = 0;
        cfg.projection_height = 0;
        cfg.car_play_source_version = String::new();
        cfg.dashboard.cluster = true;
        cfg.carplay_icon120 = Some("not base64!".into());
        let sc = stack_config(
            &cfg,
            &HashMap::new(),
            None,
            false,
            &addresses(),
            &livi_theme(),
        );
        assert_eq!(sc.info.main.width_pixels, 1920);
        assert_eq!(sc.info.main.width_physical_mm, None);
        assert!(sc.info.cluster.is_some());
        assert_eq!(sc.info.source_version, defaults().car_play_source_version);
        assert_eq!(sc.info.icons.len(), 2);
    }

    #[test]
    fn the_image_folder_gets_the_logo_and_hands_out_every_png() {
        let dir = std::env::temp_dir().join(format!("livi-carplay-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(carplay_images(&dir), [("livi".to_string(), LOGO.to_vec())]);
        std::fs::write(dir.join("frost.PNG"), b"ice").unwrap();
        std::fs::write(dir.join("notes.txt"), b"no").unwrap();
        std::fs::write(dir.join("livi.png"), b"own").unwrap();
        let names: Vec<_> = carplay_images(&dir)
            .into_iter()
            .map(|(name, png)| (name, png.len()))
            .collect();
        assert_eq!(names, [("frost".to_string(), 3), ("livi".to_string(), 3)]);
        let _ = std::fs::remove_dir_all(&dir);
        let blocked = dir.with_extension("file");
        std::fs::write(&blocked, b"").unwrap();
        assert!(carplay_images(&blocked).is_empty());
        let _ = std::fs::remove_file(&blocked);
    }

    #[test]
    fn the_cluster_page_follows_the_settings() {
        let mut cfg = defaults();
        assert_eq!(
            cluster_url(&cfg),
            "maps:/car/instrumentcluster/map?showCompass=user&showETA=yes&showSpeedLimit=user"
        );
        cfg.car_play_cluster_content = ClusterContent::Auto;
        cfg.car_play_cluster_compass = false;
        cfg.car_play_cluster_eta = false;
        cfg.car_play_cluster_speed_limit = false;
        assert_eq!(
            cluster_url(&cfg),
            "maps:/car/instrumentcluster?showCompass=no&showETA=no&showSpeedLimit=no"
        );
        cfg.car_play_cluster_content = ClusterContent::Maneuver;
        let sc = stack_config(
            &cfg,
            &HashMap::new(),
            None,
            false,
            &addresses(),
            &livi_theme(),
        );
        assert_eq!(
            sc.cluster_url,
            "maps:/car/instrumentcluster/instructioncard"
        );
        assert_eq!(sc.vehicle_variant, "");
        cfg.car_play_theme = "44C9FA31EC:985:19.0".into();
        cfg.car_play_vehicle_model = "DB12Coupe".into();
        let sc = stack_config(
            &cfg,
            &HashMap::new(),
            None,
            false,
            &addresses(),
            &livi_theme(),
        );
        assert_eq!(
            sc.vehicle_variant, "DB12Coupe",
            "the theme's car, the phone picks LHD or RHD"
        );
        let names = sc.info.theme_displays.unwrap();
        assert_eq!(
            (names.main.as_str(), names.cluster.as_str()),
            ("Center_Display", "Cluster_Display")
        );
        assert!(sc.cluster_content.is_some());
    }

    #[test]
    fn the_car_type_names_its_engines() {
        let mut cfg = defaults();
        cfg.car_type = Some(CarType::Diesel);
        cfg.max_rpm = 7000;
        cfg.redline_rpm = 9000;
        let rpm = Some(Rpm {
            max: 7000,
            redline: 7000,
        });
        assert_eq!(
            engines(&cfg),
            Engines {
                combustion: rpm,
                electric: false
            }
        );
        cfg.car_type = Some(CarType::HybridGasoline);
        assert_eq!(
            engines(&cfg),
            Engines {
                combustion: rpm,
                electric: true
            }
        );
        cfg.car_type = Some(CarType::Electric);
        assert_eq!(
            engines(&cfg),
            Engines {
                combustion: None,
                electric: true
            }
        );
        cfg.car_type = None;
        assert_eq!(engines(&cfg), Engines::default());
    }

    #[test]
    fn hevc_needs_hardware_or_no_hardware_h264() {
        let probe = |h264hw: bool, h265hw: bool, h265sw: bool| {
            format!(
                "h264={} h265={}",
                if h264hw { "hw+sw" } else { "sw" },
                match (h265hw, h265sw) {
                    (true, true) => "hw+sw",
                    (true, false) => "hw",
                    (false, true) => "sw",
                    (false, false) => "-",
                }
            )
            .parse::<Probe>()
            .unwrap()
        };
        assert!(offers_hevc(&probe(true, true, false)));
        assert!(!offers_hevc(&probe(true, false, true)));
        assert!(offers_hevc(&probe(false, false, true)));
        assert!(!offers_hevc(&Probe::default()));
    }

    #[test]
    fn the_panel_size_goes_out_as_measured() {
        assert_eq!(
            panel_mm(panels().get("main"), (1280, 720)),
            Some((300, 170))
        );
        assert_eq!(panel_mm(panels().get("cluster"), (1920, 720)), None);
        let flat = Panel {
            width_mm: 0,
            height_mm: 170,
            width_px: 1280,
            height_px: 720,
        };
        assert_eq!(panel_mm(Some(&flat), (1280, 720)), None);
    }

    #[test]
    fn the_dock_position_picks_the_status_bar_edge() {
        let mut cfg = livi_core_proto::config::defaults();
        let edge = |cfg: &Config| {
            let sc = stack_config(
                cfg,
                &HashMap::new(),
                None,
                false,
                &addresses(),
                &livi_theme(),
            );
            sc.info.main.status_bar_edge
        };
        assert_eq!(edge(&cfg), Some(1));
        cfg.car_play_dock_position = DockPosition::Side;
        assert_eq!(edge(&cfg), Some(2));
        cfg.car_play_dock_position = DockPosition::Auto;
        assert_eq!(edge(&cfg), Some(0));
    }

    #[test]
    fn the_phone_starts_with_the_saved_night_modes() {
        let mut cfg = defaults();
        cfg.main_map_night_mode = false;
        let info = stack_config(
            &cfg,
            &HashMap::new(),
            None,
            false,
            &addresses(),
            &livi_theme(),
        )
        .info;
        assert_eq!((info.main.ui_night, info.main.map_night), (true, false));
    }

    #[test]
    fn the_screen_only_counts_without_a_panel_or_dpi() {
        let screen = Panel {
            width_mm: 286,
            height_mm: 179,
            width_px: 3360,
            height_px: 2100,
        };
        let mut cfg = livi_core_proto::config::defaults();
        cfg.projection_width = 1920;
        cfg.projection_height = 1080;
        let main = |cfg: &Config, panels: &HashMap<String, Panel>| {
            stack_config(
                cfg,
                panels,
                Some(screen),
                false,
                &addresses(),
                &livi_theme(),
            )
            .info
            .main
            .width_physical_mm
        };
        assert_eq!(main(&cfg, &HashMap::new()), Some(286));
        assert_eq!(main(&cfg, &panels()), Some(300));
        cfg.screen_width_mm.main = 347;
        cfg.screen_height_mm.main = 195;
        assert_eq!(main(&cfg, &panels()), Some(347));
        cfg.screen_height_mm.main = 0;
        assert_eq!(main(&cfg, &panels()), Some(300));
        cfg.screen_height_mm.main = 195;
        cfg.projection_dpi = 160;
        assert_eq!(main(&cfg, &HashMap::new()), Some(305));
    }

    #[test]
    fn a_portrait_screen_turns_with_a_landscape_projection() {
        let screen = Panel {
            width_mm: 65,
            height_mm: 145,
            width_px: 1080,
            height_px: 2400,
        };
        let mut cfg = livi_core_proto::config::defaults();
        cfg.projection_width = 1280;
        cfg.projection_height = 576;
        let mm = |cfg: &Config| {
            let main = stack_config(
                cfg,
                &HashMap::new(),
                Some(screen),
                false,
                &addresses(),
                &livi_theme(),
            )
            .info
            .main;
            (main.width_physical_mm, main.height_physical_mm)
        };
        assert_eq!(mm(&cfg), (Some(145), Some(65)));
        cfg.projection_width = 576;
        cfg.projection_height = 1280;
        assert_eq!(mm(&cfg), (Some(65), Some(145)));
    }

    #[test]
    fn a_stream_wider_than_its_panel_gets_the_size_it_is_shown_at() {
        let monitor = Panel {
            width_mm: 340,
            height_mm: 190,
            width_px: 1920,
            height_px: 1080,
        };
        assert_eq!(panel_mm(Some(&monitor), (1920, 720)), Some((340, 127)));
        assert_eq!(panel_mm(Some(&monitor), (1920, 1080)), Some((340, 190)));
    }

    #[test]
    fn a_dpi_set_by_hand_gives_the_size() {
        assert_eq!(dpi_mm(0, 1920, 1080), None);
        assert_eq!(dpi_mm(160, 1920, 1080), Some((305, 171)));
    }
}
