use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use livi_aa_stack::config::{AaConfig, Addresses as AaAddresses, Codecs};
use livi_core_proto::config::{AppearanceMode, Config, HandDriveType};
use livi_core_proto::state::State;
use livi_cp::info::{DisplayConfig, Icon, InfoConfig, Insets};
use livi_cp::media::{AudioCodec, AudioRequest, AudioStream, Media, MicRequest, VideoStatus};
use livi_cp::stack::StackConfig;
use livi_host_proto::{PLANE_CLUSTER_RECV, PLANE_MAIN, PLANE_VIDEO, UrlStatus};
use livi_media::compositor::Panel;
use livi_media::gst_host::{self, GstHost, HostEvent};
use livi_media::planes::VideoPlane;
use tokio::sync::{broadcast, watch};

use crate::hwaddr;

const ICON_120: &str = include_str!("../../../../../src/main/shared/assets/icon-120.b64");
const ICON_180: &str = include_str!("../../../../../src/main/shared/assets/icon-180.b64");
const ICON_256: &str = include_str!("../../../../../src/main/shared/assets/icon-256.b64");

pub struct GstMedia {
    gst: GstHost,
    video: Arc<VideoPlane>,
    started: broadcast::Sender<(u32, u32)>,
    video_status: Arc<Mutex<Option<VideoStatus>>>,
}

impl GstMedia {
    pub fn new(gst: GstHost, video: Arc<VideoPlane>) -> Arc<Self> {
        let (started, _) = broadcast::channel(32);
        let mut events = gst.subscribe();
        let tx = started.clone();
        let video_status = Arc::new(Mutex::new(None));
        let latest = video_status.clone();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(HostEvent::AudioStarted { stream, first_sample }) => {
                        let _ = tx.send((stream, first_sample));
                    }
                    Ok(HostEvent::UrlStatus { plane: PLANE_VIDEO, status }) => {
                        *latest.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(video_status_of(status));
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Arc::new(Self { gst, video, started, video_status })
    }
}

impl Media for GstMedia {
    fn open_screen(&self, cluster: bool, key: [u8; 32]) -> impl Future<Output = (u16, u32)> + Send {
        let plane = if cluster { PLANE_CLUSTER_RECV } else { PLANE_MAIN };
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
        let opts = gst_host::AudioOpts {
            codec: match req.codec {
                AudioCodec::AacLc => gst_host::AudioCodec::AacLc,
                AudioCodec::Opus => gst_host::AudioCodec::Opus,
                AudioCodec::Pcm => gst_host::AudioCodec::Pcm,
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
            AudioStream { id, data_port, control_port }
        }
    }

    fn set_audio_active(&self, stream: u32, active: bool) {
        self.gst.set_audio_active(stream, active);
    }

    fn set_audio_volume(&self, stream: u32, level: f64, ramp_ms: u32) {
        self.gst.set_audio_volume(stream, level, ramp_ms);
    }

    fn close_audio(&self, stream: u32) {
        self.gst.close_audio(stream);
    }

    fn open_mic(&self, key: [u8; 32], req: MicRequest) -> u32 {
        self.gst.open_mic(
            &key,
            &gst_host::MicOpts {
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

    fn play_video(&self, url: String, audio_device: String) {
        self.forget_video_status();
        let plane = self.video.clone();
        let turn = plane.turn();
        tokio::spawn(async move {
            if plane.play(turn, &url, &audio_device).await {
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
    /// What the last player said is no word about the next one.
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
    [cfg.dashboards.dash3, cfg.dashboards.dash4].iter().any(|d| d.main || d.dash || d.aux)
}

/// Sublinear, so a resolution step moves the phone one UI size class, not two.
fn panel_mm(panels: &HashMap<String, Panel>, role: &str, w: u32, h: u32) -> Option<(i64, i64)> {
    let g = panels.get(role)?;
    if g.width_mm == 0 || g.height_mm == 0 || g.width_px == 0 || g.height_px == 0 {
        return None;
    }
    let scale = |mm: u32, px: u32, at: u32| {
        (f64::from(mm) * (f64::from(at) / f64::from(px)).powf(0.75) + 0.5).floor() as i64
    };
    let (wm, hm) = (scale(g.width_mm, g.width_px, w), scale(g.height_mm, g.height_px, h));
    (wm > 0 && hm > 0).then_some((wm, hm))
}

fn insets(top: u32, bottom: u32, left: u32, right: u32) -> Option<Insets> {
    Some(Insets { top: top.into(), bottom: bottom.into(), left: left.into(), right: right.into() })
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
        Some(Icon { width_pixels: size, height_pixels: size, png })
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addresses {
    pub device_id: String,
    pub bt_mac: String,
    pub access_point: Option<livi_wifi::OnAir>,
}

pub fn stack_config(
    cfg: &Config,
    panels: &HashMap<String, Panel>,
    hevc: bool,
    addresses: &Addresses,
) -> StackConfig {
    let or = |v: u32, d: u32| if v == 0 { d } else { v };
    let (main_w, main_h) =
        (even(or(cfg.projection_width, 1920)), even(or(cfg.projection_height, 1080)));
    let main_mm = panel_mm(panels, "main", main_w, main_h);
    let name =
        if cfg.car_name.trim().is_empty() { "LIVI".to_string() } else { cfg.car_name.clone() };
    let cluster = cluster_displayed(cfg).then(|| {
        let mm = panel_mm(panels, "cluster", cfg.cluster_width, cfg.cluster_height);
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
            ..Default::default()
        }
    });
    let source = cfg.car_play_source_version.trim();
    StackConfig {
        info: InfoConfig {
            device_name: name.clone(),
            oem_label: if cfg.oem_name.trim().is_empty() { name } else { cfg.oem_name.clone() },
            icons: icons(cfg),
            right_hand_drive: cfg.hand == HandDriveType::Rhd,
            device_id: addresses.device_id.clone(),
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
                initial_url: None,
            },
            cluster,
            entertainment_sample_rate: if cfg.sampling_frequency == 1 { 48000 } else { 44100 },
            disable_audio_output: cfg.disable_audio_output,
        },
        audio_device: cfg.audio_output_device.clone().unwrap_or_default(),
        audio_input_device: cfg.audio_input_device.clone().unwrap_or_default(),
    }
}

pub fn offers_hevc(probe: &serde_json::Value) -> bool {
    let flag = |codec: &str, kind: &str| probe[codec][kind].as_bool().unwrap_or(false);
    flag("h265", "hw") || (flag("h265", "sw") && !flag("h264", "hw"))
}

fn aa_config(cfg: &Config, codecs: Codecs, addresses: &Addresses) -> AaConfig {
    let known = |a: &str| (a != hwaddr::FALLBACK).then(|| a.to_string());
    let addresses = AaAddresses {
        bt_mac: known(&addresses.bt_mac),
        wifi_bssid: known(&addresses.device_id),
        access_point: addresses.access_point.clone(),
    };
    livi_aa_stack::config::from_livi(cfg, codecs, night_mode(cfg.appearance_mode), &addresses)
}

pub fn night_mode(mode: AppearanceMode) -> Option<bool> {
    match mode {
        AppearanceMode::Night => Some(true),
        AppearanceMode::Day => Some(false),
        AppearanceMode::Auto => None,
    }
}

pub struct ConfigSource {
    tx: watch::Sender<StackConfig>,
    aa: watch::Sender<AaConfig>,
    state: watch::Receiver<State>,
    panels: watch::Receiver<HashMap<String, Panel>>,
    codecs: Mutex<Codecs>,
    addresses: Mutex<Addresses>,
    told: Mutex<HashMap<String, String>>,
}

fn remembered(told: &mut HashMap<String, String>, key: String, found: Option<String>) -> String {
    match found {
        Some(address) => {
            told.insert(key, address.clone());
            address
        }
        None => told.get(&key).cloned().unwrap_or_else(|| hwaddr::FALLBACK.into()),
    }
}

impl ConfigSource {
    pub fn new(
        state: watch::Receiver<State>,
        panels: watch::Receiver<HashMap<String, Panel>>,
    ) -> (Arc<Self>, watch::Receiver<StackConfig>, watch::Receiver<AaConfig>) {
        let addresses = Addresses {
            device_id: hwaddr::FALLBACK.into(),
            bt_mac: hwaddr::FALLBACK.into(),
            access_point: None,
        };
        let cfg = state.borrow().config.clone();
        let first = stack_config(&cfg, &panels.borrow(), false, &addresses);
        let (tx, rx) = watch::channel(first);
        let (aa, aa_rx) = watch::channel(aa_config(&cfg, Codecs::default(), &addresses));
        let source = Arc::new(Self {
            tx,
            aa,
            state,
            panels,
            codecs: Mutex::new(Codecs::default()),
            addresses: Mutex::new(addresses),
            told: Mutex::new(HashMap::new()),
        });
        (source, rx, aa_rx)
    }

    /// Blocks on the dongle while it is asked for its access point.
    pub fn refresh(&self) {
        let cfg = self.state.borrow().config.clone();
        let found = {
            let mut told = self.told.lock().unwrap_or_else(|e| e.into_inner());
            Addresses {
                device_id: remembered(
                    &mut told,
                    format!("wifi {}", cfg.wifi_interface),
                    hwaddr::accessory_id(&cfg.wifi_interface),
                ),
                bt_mac: remembered(
                    &mut told,
                    format!("bt {}", cfg.bt_adapter),
                    hwaddr::bt_mac(&cfg.bt_adapter),
                ),
                access_point: livi_link_host::on_air(&cfg.wifi_interface),
            }
        };
        let addresses = {
            let mut known = self.addresses.lock().unwrap_or_else(|e| e.into_inner());
            if *known != found {
                if (&known.device_id, &known.bt_mac) != (&found.device_id, &found.bt_mac) {
                    println!("[core] CarPlay accessory {} (bt {})", found.device_id, found.bt_mac);
                }
                *known = found;
            }
            known.clone()
        };
        let codecs = *self.codecs.lock().unwrap_or_else(|e| e.into_inner());
        let next = stack_config(&cfg, &self.panels.borrow(), codecs.hevc, &addresses);
        self.tx.send_if_modified(|cur| {
            let changed = *cur != next;
            *cur = next;
            changed
        });
        livi_aa_stack::log::set_debug(cfg.debug_logging);
        crate::log_file::set_debug(cfg.debug_logging);
        let aa = aa_config(&cfg, codecs, &addresses);
        self.aa.send_if_modified(|cur| {
            let changed = *cur != aa;
            *cur = aa;
            changed
        });
    }

    pub fn set_codecs(&self, codecs: Codecs) {
        println!("[core] codecs hevc {} vp9 {} av1 {}", codecs.hevc, codecs.vp9, codecs.av1);
        *self.codecs.lock().unwrap_or_else(|e| e.into_inner()) = codecs;
    }

    /// Telemetry changes the state many times a second, the addresses are not asked again for
    /// that.
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

    use super::*;

    fn panels() -> HashMap<String, Panel> {
        HashMap::from([(
            "main".to_string(),
            Panel { width_mm: 300, height_mm: 170, width_px: 1280, height_px: 720 },
        )])
    }

    fn addresses() -> Addresses {
        Addresses {
            device_id: "11:11:11:11:11:11".into(),
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
        cfg.dashboards.dash3.main = false;
        cfg.dashboards.dash4.main = false;
        let sc = stack_config(&cfg, &panels(), true, &addresses());
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
        assert_eq!(remembered(&mut told, "wifi livi-link".into(), None), hwaddr::FALLBACK);
        let mac = Some("88:00:33:77:8C:26".to_string());
        assert_eq!(remembered(&mut told, "wifi livi-link".into(), mac), "88:00:33:77:8C:26");
        assert_eq!(remembered(&mut told, "wifi livi-link".into(), None), "88:00:33:77:8C:26");
        assert_eq!(remembered(&mut told, "wifi wlan0".into(), None), hwaddr::FALLBACK);
    }

    #[test]
    fn a_cluster_dashboard_brings_the_second_screen() {
        let mut cfg = defaults();
        cfg.projection_width = 0;
        cfg.projection_height = 0;
        cfg.car_play_source_version = String::new();
        cfg.dashboards.dash3.dash = true;
        cfg.carplay_icon120 = Some("not base64!".into());
        let sc = stack_config(&cfg, &HashMap::new(), false, &addresses());
        assert_eq!(sc.info.main.width_pixels, 1920);
        assert_eq!(sc.info.main.width_physical_mm, None);
        assert!(sc.info.cluster.is_some());
        assert_eq!(sc.info.source_version, defaults().car_play_source_version);
        assert_eq!(sc.info.icons.len(), 2);
    }

    #[test]
    fn hevc_needs_hardware_or_no_hardware_h264() {
        let probe = |h264hw: bool, h265hw: bool, h265sw: bool| serde_json::json!({ "h264": { "hw": h264hw, "sw": true }, "h265": { "hw": h265hw, "sw": h265sw } });
        assert!(offers_hevc(&probe(true, true, false)));
        assert!(!offers_hevc(&probe(true, false, true)));
        assert!(offers_hevc(&probe(false, false, true)));
        assert!(!offers_hevc(&serde_json::json!({})));
    }

    #[test]
    fn panels_scale_sublinearly() {
        assert_eq!(panel_mm(&panels(), "main", 1280, 720), Some((300, 170)));
        assert_eq!(panel_mm(&panels(), "main", 2560, 1440), Some((505, 286)));
        assert_eq!(panel_mm(&panels(), "cluster", 800, 480), None);
        let flat = HashMap::from([(
            "main".to_string(),
            Panel { width_mm: 0, height_mm: 170, width_px: 1280, height_px: 720 },
        )]);
        assert_eq!(panel_mm(&flat, "main", 1280, 720), None);
    }
}
