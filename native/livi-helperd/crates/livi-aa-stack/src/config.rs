use livi_core_proto::config::{CarType, Config, HandDriveType};
use livi_wifi::{OnAir, Security};

use crate::wire::round_half_up;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AaConfig {
    pub hu_name: Option<String>,
    /// The tier the phone encodes into.
    pub video_width: Option<u32>,
    pub video_height: Option<u32>,
    pub video_dpi: Option<u32>,
    pub video_fps: Option<u32>,
    pub pixel_aspect_ratio_e4: Option<u32>,
    /// The physical screen the tier is shown on.
    pub display_width: u32,
    pub display_height: u32,
    /// The view area becomes margins, the safe area content insets.
    pub main_view_area: Insets,
    pub main_safe_area: Insets,
    /// 1 for right-hand drive.
    pub driver_position: u8,
    pub bt_mac_address: Option<String>,
    pub wifi_bssid: Option<String>,
    pub wifi_ssid: String,
    pub wifi_password: String,
    pub wifi_security: Security,
    pub wifi_channel: Option<u32>,
    pub fuel_types: Vec<i32>,
    pub ev_connector_types: Vec<i32>,
    pub hevc_supported: bool,
    pub vp9_supported: bool,
    pub av1_supported: bool,
    pub initial_night_mode: Option<bool>,
    pub cluster_enabled: bool,
    pub cluster_width: u32,
    pub cluster_height: u32,
    pub cluster_tier_width: Option<u32>,
    pub cluster_tier_height: Option<u32>,
    pub cluster_pixel_aspect_ratio_e4: Option<u32>,
    pub cluster_fps: u32,
    pub cluster_dpi: Option<u32>,
    pub cluster_view_area: Insets,
    pub cluster_safe_area: Insets,
    pub disable_audio_output: bool,
    /// Empty for the default capture device.
    pub mic_device: String,
    /// Offers the phone a call audio channel next to hands-free, `LIVI_AA_TELEPHONY=1`.
    pub telephony_audio: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Insets {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Codecs {
    pub hevc: bool,
    pub vp9: bool,
    pub av1: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Addresses {
    pub bt_mac: Option<String>,
    pub wifi_bssid: Option<String>,
    /// The access point a phone joins, when one is on air.
    pub access_point: Option<OnAir>,
}

const SQUARE_PIXEL_E4: u32 = 10000;

const AA_TIERS: [(u32, u32); 5] =
    [(800, 480), (1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
/// Phones refuse a tier above 1080p unless a codec other than h264 is offered.
const H264_MAX_TIER_WIDTH: u32 = 1920;
const MAX_TIER_UPSCALE: f64 = 1.2;

fn round_even(n: f64) -> f64 {
    (n.floor() as i64 & !1).max(2) as f64
}

pub fn content_area(frame: (u32, u32), user: (u32, u32)) -> (f64, f64) {
    let user_ar = f64::from(user.0.max(1)) / f64::from(user.1.max(1));
    let frame_ar = f64::from(frame.0.max(1)) / f64::from(frame.1.max(1));
    if user_ar <= frame_ar {
        (round_even(f64::from(frame.1) * user_ar), f64::from(frame.1))
    } else {
        (f64::from(frame.0), round_even(f64::from(frame.0) / user_ar))
    }
}

pub fn fitting_tier(width: u32, height: u32, h264_only: bool) -> (u32, u32) {
    let (user_w, user_h) = (width.max(1), height.max(1));
    let max_w = if h264_only { H264_MAX_TIER_WIDTH } else { u32::MAX };
    let mut chosen = AA_TIERS[0];
    for tier in AA_TIERS {
        if tier.0 > max_w {
            break;
        }
        chosen = tier;
        let (cw, ch) = content_area(tier, (user_w, user_h));
        let upscale = (f64::from(user_w) / cw).max(f64::from(user_h) / ch);
        if upscale <= MAX_TIER_UPSCALE {
            break;
        }
    }
    chosen
}

/// Densities of a typical car screen at each tier.
const DPI_TIERS: [(u64, f64); 5] = [
    (800 * 480, 140.0),
    (1280 * 720, 180.0),
    (1920 * 1080, 200.0),
    (2560 * 1440, 250.0),
    (3840 * 2160, 420.0),
];

pub fn android_auto_dpi(width: u32, height: u32) -> u32 {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= DPI_TIERS[0].0 {
        return DPI_TIERS[0].1 as u32;
    }
    let top = DPI_TIERS[DPI_TIERS.len() - 1];
    if pixels >= top.0 {
        return top.1 as u32;
    }
    let mut i = 0;
    while pixels > DPI_TIERS[i + 1].0 {
        i += 1;
    }
    let (lo, hi) = (DPI_TIERS[i], DPI_TIERS[i + 1]);
    let t = (pixels - lo.0) as f64 / (hi.0 - lo.0) as f64;
    let dpi = lo.1 + t * (hi.1 - lo.1);
    (round_half_up(dpi / 10.0) * 10.0) as u32
}

fn fuel_types(car: Option<CarType>) -> Vec<i32> {
    match car {
        Some(CarType::HybridGasoline) => {
            vec![CarType::Gasoline as i32, CarType::Electric as i32]
        }
        Some(CarType::HybridDiesel) => vec![CarType::Diesel as i32, CarType::Electric as i32],
        None | Some(CarType::Unknown) => vec![CarType::Gasoline as i32],
        Some(other) => vec![other as i32],
    }
}

/// dash3 and dash4 are the cluster dashboards.
pub fn cluster_displayed(cfg: &Config) -> bool {
    [cfg.dashboards.dash3, cfg.dashboards.dash4].iter().any(|d| d.main || d.dash || d.aux)
}

pub fn from_livi(
    cfg: &Config,
    codecs: Codecs,
    initial_night_mode: Option<bool>,
    addresses: &Addresses,
) -> AaConfig {
    let h264_only = !(codecs.hevc || codecs.vp9 || codecs.av1);
    let (tier_w, tier_h) = fitting_tier(cfg.projection_width, cfg.projection_height, h264_only);
    let dpi =
        if cfg.projection_dpi > 0 { cfg.projection_dpi } else { android_auto_dpi(tier_w, tier_h) };
    let (cluster_tier_w, cluster_tier_h) =
        fitting_tier(cfg.cluster_width, cfg.cluster_height, h264_only);
    let cluster_dpi = if cfg.cluster_dpi > 0 {
        cfg.cluster_dpi
    } else {
        android_auto_dpi(cluster_tier_w, cluster_tier_h)
    };
    let name =
        if cfg.car_name.trim().is_empty() { "LIVI".to_string() } else { cfg.car_name.clone() };
    let cluster = cluster_displayed(cfg);
    println!(
        "[AaSession] display {}x{} -> AA tier {tier_w}x{tier_h} @{dpi}dpi, cluster {}",
        cfg.projection_width,
        cfg.projection_height,
        if cluster {
            format!(
                "{}x{} -> tier {cluster_tier_w}x{cluster_tier_h} @{cluster_dpi}dpi",
                cfg.cluster_width, cfg.cluster_height
            )
        } else {
            "not advertised".to_string()
        }
    );
    let on_air = addresses.access_point.as_ref();
    AaConfig {
        hu_name: Some(name.clone()),
        video_width: Some(tier_w),
        video_height: Some(tier_h),
        video_dpi: Some(dpi),
        video_fps: Some(if cfg.projection_fps == 60 { 60 } else { 30 }),
        pixel_aspect_ratio_e4: Some(SQUARE_PIXEL_E4),
        display_width: cfg.projection_width,
        display_height: cfg.projection_height,
        main_view_area: Insets {
            top: cfg.projection_view_area_top,
            bottom: cfg.projection_view_area_bottom,
            left: cfg.projection_view_area_left,
            right: cfg.projection_view_area_right,
        },
        main_safe_area: Insets {
            top: cfg.projection_safe_area_top,
            bottom: cfg.projection_safe_area_bottom,
            left: cfg.projection_safe_area_left,
            right: cfg.projection_safe_area_right,
        },
        driver_position: u8::from(cfg.hand == HandDriveType::Rhd),
        bt_mac_address: addresses.bt_mac.clone(),
        wifi_bssid: addresses.wifi_bssid.clone(),
        wifi_ssid: on_air.map(|ap| ap.ssid.clone()).filter(|s| !s.is_empty()).unwrap_or(name),
        wifi_password: if cfg.wifi_password.is_empty() {
            "12345678".to_string()
        } else {
            cfg.wifi_password.clone()
        },
        wifi_security: on_air.map(|ap| ap.security).unwrap_or_default(),
        wifi_channel: Some(on_air.map_or(cfg.wifi_channel, |ap| ap.channel.number)),
        fuel_types: fuel_types(cfg.car_type),
        ev_connector_types: cfg.ev_connector_types.iter().flatten().map(|t| *t as i32).collect(),
        hevc_supported: codecs.hevc,
        vp9_supported: codecs.vp9,
        av1_supported: codecs.av1,
        initial_night_mode,
        cluster_enabled: cluster,
        cluster_width: cfg.cluster_width,
        cluster_height: cfg.cluster_height,
        cluster_tier_width: Some(cluster_tier_w),
        cluster_tier_height: Some(cluster_tier_h),
        cluster_pixel_aspect_ratio_e4: Some(SQUARE_PIXEL_E4),
        cluster_fps: cfg.cluster_fps,
        cluster_dpi: Some(cluster_dpi),
        cluster_view_area: Insets {
            top: cfg.cluster_view_area_top,
            bottom: cfg.cluster_view_area_bottom,
            left: cfg.cluster_view_area_left,
            right: cfg.cluster_view_area_right,
        },
        cluster_safe_area: Insets {
            top: cfg.cluster_safe_area_top,
            bottom: cfg.cluster_safe_area_bottom,
            left: cfg.cluster_safe_area_left,
            right: cfg.cluster_safe_area_right,
        },
        disable_audio_output: cfg.disable_audio_output,
        mic_device: cfg.audio_input_device.clone().unwrap_or_default(),
        telephony_audio: std::env::var("LIVI_AA_TELEPHONY").is_ok_and(|v| v == "1"),
    }
}

pub fn aspect_margins(screen: (u32, u32), tier: (u32, u32)) -> (u32, u32) {
    let (sw, sh) = (f64::from(screen.0), f64::from(screen.1));
    let (tw, th) = (i64::from(tier.0), i64::from(tier.1));
    if screen.0 == 0 || screen.1 == 0 || tw <= 0 || th <= 0 {
        return (0, 0);
    }
    let screen_ar = sw / sh;
    let tier_ar = tw as f64 / th as f64;
    if screen_ar > tier_ar {
        let content_h = round_half_up(tw as f64 / screen_ar) as i64 & !1;
        (0, (th - content_h).max(0) as u32)
    } else if screen_ar < tier_ar {
        let content_w = round_half_up(th as f64 * screen_ar) as i64 & !1;
        ((tw - content_w).max(0) as u32, 0)
    } else {
        (0, 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub tier_width: u32,
    pub tier_height: u32,
    pub width_margin: u32,
    pub height_margin: u32,
    pub inset: Insets,
}

impl Geometry {
    pub fn new(tier: (u32, u32), screen: (u32, u32), view: Insets) -> Self {
        let (wm, hm) = aspect_margins(screen, tier);
        Self {
            tier_width: tier.0,
            tier_height: tier.1,
            width_margin: wm,
            height_margin: hm,
            inset: Insets {
                top: hm / 2 + view.top,
                bottom: hm - hm / 2 + view.bottom,
                left: wm / 2 + view.left,
                right: wm - wm / 2 + view.right,
            },
        }
    }

    pub fn main(cfg: &AaConfig) -> Self {
        let tier = (cfg.video_width.unwrap_or(1280), cfg.video_height.unwrap_or(720));
        Self::new(tier, (cfg.display_width, cfg.display_height), cfg.main_view_area)
    }

    pub fn touch_size(&self) -> (i64, i64) {
        let w =
            i64::from(self.tier_width) - i64::from(self.inset.left) - i64::from(self.inset.right);
        let h =
            i64::from(self.tier_height) - i64::from(self.inset.top) - i64::from(self.inset.bottom);
        (w.max(1), h.max(1))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn tiers_and_densities() {
        assert_eq!(fitting_tier(800, 480, true), (800, 480));
        assert_eq!(fitting_tier(1024, 600, true), (1280, 720));
        assert_eq!(fitting_tier(1920, 720, true), (1920, 1080));
        assert_eq!(fitting_tier(3840, 2160, true), (1920, 1080));
        assert_eq!(fitting_tier(3840, 2160, false), (3840, 2160));
        assert_eq!(fitting_tier(0, 0, true), (800, 480));
        assert_eq!(android_auto_dpi(800, 480), 140);
        assert_eq!(android_auto_dpi(1280, 720), 180);
        assert_eq!(android_auto_dpi(1600, 900), 190);
        assert_eq!(android_auto_dpi(3840, 2160), 420);
        assert_eq!(android_auto_dpi(100, 100), 140);
        assert_eq!(fuel_types(Some(CarType::HybridDiesel)), [4, 10]);
        assert_eq!(fuel_types(Some(CarType::HybridGasoline)), [1, 10]);
        assert_eq!(fuel_types(None), [1]);
        assert_eq!(fuel_types(Some(CarType::Electric)), [10]);
    }

    #[test]
    fn the_letterbox_goes_on_the_short_side() {
        assert_eq!(aspect_margins((1920, 720), (1920, 1080)), (0, 360));
        assert_eq!(aspect_margins((1024, 768), (1280, 720)), (320, 0));
        assert_eq!(aspect_margins((1280, 720), (1280, 720)), (0, 0));
        assert_eq!(aspect_margins((0, 720), (1280, 720)), (0, 0));
        let g = Geometry::new(
            (1920, 1080),
            (1920, 720),
            Insets { top: 1, bottom: 2, left: 3, right: 4 },
        );
        assert_eq!(g.inset, Insets { top: 181, bottom: 182, left: 3, right: 4 });
        assert_eq!(g.touch_size(), (1913, 717));
    }

    #[test]
    fn livi_settings_become_the_session_config() {
        let mut cfg = livi_core_proto::config::defaults();
        cfg.projection_width = 1920;
        cfg.projection_height = 720;
        cfg.projection_dpi = 0;
        cfg.car_name = "  ".into();
        cfg.wifi_password = String::new();
        cfg.hand = HandDriveType::Rhd;
        cfg.car_type = Some(CarType::HybridGasoline);
        cfg.dashboards.dash3.main = true;
        let addresses = Addresses {
            bt_mac: Some("AA:BB:CC:DD:EE:FF".into()),
            wifi_bssid: None,
            access_point: None,
        };
        let aa = from_livi(&cfg, Codecs::default(), Some(true), &addresses);
        assert_eq!(aa.hu_name.as_deref(), Some("LIVI"));
        assert_eq!(aa.wifi_ssid, "LIVI");
        assert_eq!(aa.wifi_password, "12345678");
        assert_eq!((aa.video_width, aa.video_height), (Some(1920), Some(1080)));
        assert_eq!(aa.video_dpi, Some(200));
        assert_eq!(aa.driver_position, 1);
        assert_eq!(aa.fuel_types, [1, 10]);
        assert!(aa.cluster_enabled);
        assert_eq!(aa.initial_night_mode, Some(true));
        assert_eq!(aa.bt_mac_address.as_deref(), Some("AA:BB:CC:DD:EE:FF"));
        cfg.dashboards.dash3.main = false;
        cfg.dashboards.dash4.main = false;
        cfg.dashboards.dash4.dash = false;
        cfg.dashboards.dash4.aux = false;
        cfg.dashboards.dash3.dash = false;
        cfg.dashboards.dash3.aux = false;
        assert!(!from_livi(&cfg, Codecs::default(), None, &addresses).cluster_enabled);
    }
}
