use livi_aa_proto::{
    AudioConfiguration, AudioStreamType, BluetoothPairingMethod, BluetoothService, CarInfo,
    DisplayInsets, DisplayType, InputSourceService, Keycode, MediaCodecType,
    MediaPlaybackStatusService, MediaSinkService, MediaSourceService, NavigationClusterType,
    NavigationImageOptions, NavigationStatusService, PhoneStatusService, SensorSourceService,
    SensorType, Service, ServiceDiscoveryResponse, SupportedSensor, Touchscreen, UiConfig,
    VideoCodecResolution, VideoConfiguration, VideoFrameRate, WifiProjectionService,
};

use crate::codec::encode;
use crate::config::{AaConfig, Geometry, Insets};
use crate::consts::ch;
use crate::log::{debug, hex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    H264,
    H265,
    Vp9,
    Av1,
}

impl VideoCodec {
    pub fn name(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Vp9 => "vp9",
            Self::Av1 => "av1",
        }
    }

    pub fn of_media_codec(codec: i32) -> Self {
        match MediaCodecType::try_from(codec) {
            Ok(MediaCodecType::VideoH265) => Self::H265,
            Ok(MediaCodecType::VideoVp9) => Self::Vp9,
            Ok(MediaCodecType::VideoAv1) => Self::Av1,
            _ => Self::H264,
        }
    }

    fn media_codec(self) -> i32 {
        let codec = match self {
            Self::H264 => MediaCodecType::VideoH264Bp,
            Self::H265 => MediaCodecType::VideoH265,
            Self::Vp9 => MediaCodecType::VideoVp9,
            Self::Av1 => MediaCodecType::VideoAv1,
        };
        codec as i32
    }

    /// From protocol 5.0 on the phone drops a display that offers more than one codec type, and
    /// it only ever encodes H.264 or H.265.
    fn for_display(cfg: &AaConfig) -> Self {
        if cfg.hevc_supported { Self::H265 } else { Self::H264 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    pub buf: Vec<u8>,
    /// Indexed like the offered video configurations.
    pub video_codecs: Vec<VideoCodec>,
    pub cluster_codecs: Vec<VideoCodec>,
}

/// Raw GPS plus accelerometer, gyroscope, compass and car speed.
const LOCATION_CHARACTERIZATION: u32 = 256 | 4 | 2 | 8 | 64;

const KEYCODES: [Keycode; 46] = [
    Keycode::Home,
    Keycode::Back,
    Keycode::Call,
    Keycode::Endcall,
    Keycode::Keycode0,
    Keycode::Keycode1,
    Keycode::Keycode2,
    Keycode::Keycode3,
    Keycode::Keycode4,
    Keycode::Keycode5,
    Keycode::Keycode6,
    Keycode::Keycode7,
    Keycode::Keycode8,
    Keycode::Keycode9,
    Keycode::Star,
    Keycode::Pound,
    Keycode::DpadUp,
    Keycode::DpadDown,
    Keycode::DpadLeft,
    Keycode::DpadRight,
    Keycode::DpadCenter,
    Keycode::VolumeUp,
    Keycode::VolumeDown,
    Keycode::Power,
    Keycode::Enter,
    Keycode::Headsethook,
    Keycode::Menu,
    Keycode::Search,
    Keycode::MediaPlayPause,
    Keycode::MediaStop,
    Keycode::MediaNext,
    Keycode::MediaPrevious,
    Keycode::MediaRewind,
    Keycode::MediaFastForward,
    Keycode::Mute,
    Keycode::Escape,
    Keycode::MediaPlay,
    Keycode::MediaPause,
    Keycode::VolumeMute,
    Keycode::Assist,
    Keycode::VoiceAssist,
    Keycode::NavigatePrevious,
    Keycode::NavigateNext,
    Keycode::NavigateIn,
    Keycode::NavigateOut,
    Keycode::RotaryController,
];

const SENSORS: [SensorType; 21] = [
    SensorType::DrivingStatusData,
    SensorType::Location,
    SensorType::NightMode,
    SensorType::Speed,
    SensorType::Gear,
    SensorType::ParkingBrake,
    SensorType::Fuel,
    SensorType::Odometer,
    SensorType::EnvironmentData,
    SensorType::DoorData,
    SensorType::LightData,
    SensorType::TirePressureData,
    SensorType::HvacData,
    SensorType::AccelerometerData,
    SensorType::GyroscopeData,
    SensorType::Compass,
    SensorType::GpsSatelliteData,
    SensorType::Rpm,
    SensorType::VehicleEnergyModelData,
    SensorType::RawVehicleEnergyModel,
    SensorType::RawEvTripSettings,
];

fn resolution(width: u32) -> VideoCodecResolution {
    use VideoCodecResolution as R;
    if width >= 3840 {
        R::VideoCodecResolution3840x2160
    } else if width >= 2560 {
        R::VideoCodecResolution2560x1440
    } else if width >= 1920 {
        R::VideoCodecResolution1920x1080
    } else if width <= 800 {
        R::VideoCodecResolution800x480
    } else {
        R::VideoCodecResolution1280x720
    }
}

fn resolution_of(width: u32, height: u32) -> Option<VideoCodecResolution> {
    use VideoCodecResolution as R;
    match (width, height) {
        (800, 480) => Some(R::VideoCodecResolution800x480),
        (1280, 720) => Some(R::VideoCodecResolution1280x720),
        (1920, 1080) => Some(R::VideoCodecResolution1920x1080),
        _ => None,
    }
}

fn frame_rate(fps: u32) -> VideoFrameRate {
    if fps == 60 { VideoFrameRate::VideoFrameRate60 } else { VideoFrameRate::VideoFrameRate30 }
}

fn display_insets(i: Insets) -> DisplayInsets {
    DisplayInsets {
        top: Some(i.top),
        bottom: Some(i.bottom),
        left: Some(i.left),
        right: Some(i.right),
    }
}

fn audio(sampling_rate_hz: u32, channel_count: u32) -> AudioConfiguration {
    AudioConfiguration { sampling_rate_hz, bits_per_sample: 16, channel_count }
}

fn audio_sink(id: u8, stream: i32, sampling_rate: u32, channels: u32) -> Service {
    Service {
        id: i32::from(id),
        media_sink: Some(MediaSinkService {
            codec_type: Some(MediaCodecType::AudioPcm as i32),
            audio_stream_type: Some(stream),
            audio_configurations: vec![audio(sampling_rate, channels)],
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn video_configs(base: &VideoConfiguration, codecs: &[VideoCodec]) -> Vec<VideoConfiguration> {
    codecs
        .iter()
        .map(|c| VideoConfiguration { codec_type: Some(c.media_codec()), ..base.clone() })
        .collect()
}

/// The car's identity goes in the flat fields as well, the phone reads them when car_info is
/// missing.
pub fn build(cfg: &AaConfig) -> Discovery {
    let v_w = cfg.video_width.unwrap_or(1280);
    let dpi = cfg.video_dpi.unwrap_or(140);
    let v_res = resolution(v_w);
    let v_fps = frame_rate(cfg.video_fps.unwrap_or(30));
    let main = Geometry::main(cfg);
    let main_content = display_insets(cfg.main_safe_area);

    let mut channels = Vec::new();

    let base = VideoConfiguration {
        codec_resolution: Some(v_res as i32),
        frame_rate: Some(v_fps as i32),
        margin_width: Some(main.width_margin),
        margin_height: Some(main.height_margin),
        density_dpi: Some(dpi),
        pixel_aspect_ratio_e4: Some(cfg.pixel_aspect_ratio_e4.unwrap_or(10000)),
        ui_config: Some(UiConfig {
            margins: Some(display_insets(main.inset)),
            content_insets: Some(main_content),
            stable_content_insets: Some(main_content),
            ..Default::default()
        }),
        ..Default::default()
    };
    let codec = VideoCodec::for_display(cfg);
    let video_codecs = vec![codec];
    if debug() {
        println!("[Session] advertising codec: {}", codec.name());
    }
    channels.push(Service {
        id: i32::from(ch::VIDEO),
        media_sink: Some(MediaSinkService {
            codec_type: Some(codec.media_codec()),
            video_configurations: video_configs(&base, &video_codecs),
            ..Default::default()
        }),
        ..Default::default()
    });

    let mut cluster_codecs = Vec::new();
    if cfg.cluster_enabled {
        let (c_w, c_h) = (cfg.cluster_width, cfg.cluster_height);
        let tier = (cfg.cluster_tier_width.unwrap_or(c_w), cfg.cluster_tier_height.unwrap_or(c_h));
        let cluster_res = resolution_of(tier.0, tier.1).unwrap_or(v_res);
        let cluster_fps = match cfg.cluster_fps {
            30 | 60 => frame_rate(cfg.cluster_fps),
            _ => v_fps,
        };
        let geometry = Geometry::new(tier, (c_w, c_h), cfg.cluster_view_area);
        let content = display_insets(cfg.cluster_safe_area);
        let base = VideoConfiguration {
            codec_resolution: Some(cluster_res as i32),
            frame_rate: Some(cluster_fps as i32),
            margin_width: Some(geometry.width_margin),
            margin_height: Some(geometry.height_margin),
            density_dpi: Some(cfg.cluster_dpi.unwrap_or(dpi)),
            pixel_aspect_ratio_e4: Some(cfg.cluster_pixel_aspect_ratio_e4.unwrap_or(10000)),
            ui_config: Some(UiConfig {
                margins: Some(display_insets(geometry.inset)),
                content_insets: Some(content),
                stable_content_insets: Some(content),
                ..Default::default()
            }),
            ..Default::default()
        };
        cluster_codecs.push(codec);
        channels.push(Service {
            id: i32::from(ch::CLUSTER_VIDEO),
            media_sink: Some(MediaSinkService {
                codec_type: Some(codec.media_codec()),
                video_configurations: video_configs(&base, &cluster_codecs),
                display_type: Some(DisplayType::Cluster as i32),
                display_id: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        });
        channels.push(Service {
            id: i32::from(ch::CLUSTER_INPUT),
            input_source: Some(InputSourceService { display_id: Some(1), ..Default::default() }),
            ..Default::default()
        });
    }

    if !cfg.disable_audio_output {
        channels.push(audio_sink(ch::MEDIA_AUDIO, AudioStreamType::Media as i32, 48000, 2));
        channels.push(audio_sink(ch::SPEECH_AUDIO, AudioStreamType::Guidance as i32, 16000, 1));
        if cfg.telephony_audio {
            // Probe: LIVI_AA_TELEPHONY_TYPE puts another stream type on the same channel.
            let stream = std::env::var("LIVI_AA_TELEPHONY_TYPE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(AudioStreamType::Telephony as i32);
            channels.push(audio_sink(ch::TELEPHONY_AUDIO, stream, 16000, 1));
        }
    }
    channels.push(audio_sink(ch::SYSTEM_AUDIO, AudioStreamType::SystemAudio as i32, 16000, 1));
    channels.push(Service {
        id: i32::from(ch::MIC_INPUT),
        media_source: Some(MediaSourceService {
            codec_type: Some(MediaCodecType::AudioPcm as i32),
            audio_configuration: Some(audio(16000, 1)),
        }),
        ..Default::default()
    });

    let fuel_types = if cfg.fuel_types.is_empty() { vec![1] } else { cfg.fuel_types.clone() };
    channels.push(Service {
        id: i32::from(ch::SENSOR),
        sensor_source: Some(SensorSourceService {
            supported_sensors: SENSORS
                .iter()
                .map(|t| SupportedSensor { sensor_type: *t as i32 })
                .collect(),
            location_characterization: Some(LOCATION_CHARACTERIZATION),
            fuel_types,
            ev_connector_types: cfg.ev_connector_types.clone(),
        }),
        ..Default::default()
    });

    let (touch_w, touch_h) = main.touch_size();
    channels.push(Service {
        id: i32::from(ch::INPUT),
        input_source: Some(InputSourceService {
            supported_keycodes: KEYCODES.iter().map(|k| *k as i32).collect(),
            touchscreens: vec![Touchscreen {
                width: touch_w as i32,
                height: touch_h as i32,
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });

    channels.push(Service {
        id: i32::from(ch::BLUETOOTH),
        bluetooth: Some(BluetoothService {
            car_address: cfg
                .bt_mac_address
                .clone()
                .unwrap_or_else(|| "00:00:00:00:00:00".to_string()),
            supported_pairing_methods: vec![
                BluetoothPairingMethod::Pin as i32,
                BluetoothPairingMethod::NumericComparison as i32,
            ],
        }),
        ..Default::default()
    });

    channels.push(Service {
        id: i32::from(ch::NAVIGATION),
        navigation_status: Some(NavigationStatusService {
            minimum_interval_ms: 500,
            cluster_type: NavigationClusterType::Image as i32,
            image_options: Some(NavigationImageOptions {
                height: 256,
                width: 256,
                color_depth_bits: 32,
            }),
        }),
        ..Default::default()
    });
    channels.push(Service {
        id: i32::from(ch::MEDIA_INFO),
        media_playback: Some(MediaPlaybackStatusService {}),
        ..Default::default()
    });
    channels.push(Service {
        id: i32::from(ch::PHONE_STATUS),
        phone_status: Some(PhoneStatusService {}),
        ..Default::default()
    });
    if let Some(bssid) = cfg.wifi_bssid.as_ref().filter(|b| !b.is_empty()) {
        channels.push(Service {
            id: i32::from(ch::WIFI),
            wifi_projection: Some(WifiProjectionService { car_wifi_bssid: Some(bssid.clone()) }),
            ..Default::default()
        });
    }

    let make = || Some("LIVI".to_string());
    let model = || Some("Universal".to_string());
    let year = || Some("2026".to_string());
    let vehicle = || Some("livi-001".to_string());
    let hu_model = || Some("LIVI Head Unit".to_string());
    let build_no = || Some("1".to_string());
    let version = || Some("1.0".to_string());
    let channel_count = channels.len();
    let sdr = ServiceDiscoveryResponse {
        services: channels,
        manufacturer: make(),
        model: model(),
        model_year: year(),
        vehicle_id: vehicle(),
        driver_position: Some(i32::from(cfg.driver_position)),
        head_unit_make: make(),
        head_unit_model: hu_model(),
        head_unit_software_build: build_no(),
        head_unit_software_version: version(),
        can_play_native_media_during_vr: Some(true),
        session_flags: None,
        display_name: Some(cfg.hu_name.clone().unwrap_or_else(|| "LIVI".to_string())),
        probe_only: Some(false),
        car_info: Some(CarInfo {
            manufacturer: make(),
            model: model(),
            model_year: year(),
            vehicle_id: vehicle(),
            head_unit_make: make(),
            head_unit_model: hu_model(),
            head_unit_software_build: build_no(),
            head_unit_software_version: version(),
            vehicle_type: None,
        }),
    };
    let buf = encode(&sdr);
    if debug() {
        println!("[Session] SDR: {channel_count} channels, {}B", buf.len());
        let shown = &buf[..buf.len().min(64)];
        println!("[Session] SDR hex: {}{}", hex(shown), if buf.len() > 64 { "..." } else { "" });
    }
    Discovery { buf, video_codecs, cluster_codecs }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    fn decoded(cfg: &AaConfig) -> ServiceDiscoveryResponse {
        ServiceDiscoveryResponse::decode(build(cfg).buf.as_slice()).unwrap()
    }

    fn ids(sdr: &ServiceDiscoveryResponse) -> Vec<i32> {
        sdr.services.iter().map(|c| c.id).collect()
    }

    #[test]
    fn the_default_offer() {
        let d = build(&AaConfig::default());
        assert_eq!(d.video_codecs, [VideoCodec::H264]);
        assert!(d.cluster_codecs.is_empty());
        let sdr = decoded(&AaConfig::default());
        assert_eq!(ids(&sdr), [3, 4, 5, 6, 9, 1, 8, 10, 12, 13, 14]);
        assert_eq!(sdr.display_name.as_deref(), Some("LIVI"));
        let bt = sdr.services[7].bluetooth.as_ref().unwrap();
        assert_eq!(bt.car_address, "00:00:00:00:00:00");
        let touch = &sdr.services[6].input_source.as_ref().unwrap().touchscreens[0];
        assert_eq!((touch.width, touch.height), (1280, 720));
    }

    #[test]
    fn call_audio_is_offered_only_when_asked() {
        let sdr = decoded(&AaConfig { telephony_audio: true, ..Default::default() });
        assert_eq!(ids(&sdr), [3, 4, 5, 7, 6, 9, 1, 8, 10, 12, 13, 14]);
        let call = sdr.services[3].media_sink.as_ref().unwrap();
        assert_eq!(call.audio_stream_type, Some(AudioStreamType::Telephony as i32));
        let muted =
            AaConfig { telephony_audio: true, disable_audio_output: true, ..Default::default() };
        assert!(!ids(&decoded(&muted)).contains(&7));
    }

    #[test]
    fn codecs_cluster_and_audio_follow_the_config() {
        let cfg = AaConfig {
            hevc_supported: true,
            vp9_supported: true,
            av1_supported: true,
            cluster_enabled: true,
            cluster_width: 800,
            cluster_height: 400,
            cluster_fps: 60,
            disable_audio_output: true,
            wifi_bssid: Some("11:22:33:44:55:66".into()),
            ..Default::default()
        };
        let d = build(&cfg);
        assert_eq!(d.video_codecs, [VideoCodec::H265]);
        assert_eq!(d.cluster_codecs, [VideoCodec::H265]);
        let sdr = decoded(&cfg);
        assert_eq!(ids(&sdr), [3, 19, 20, 6, 9, 1, 8, 10, 12, 13, 14, 18]);
        for display in &sdr.services[..2] {
            let sink = display.media_sink.as_ref().unwrap();
            assert_eq!(sink.codec_type, Some(MediaCodecType::VideoH265 as i32));
            assert!(sink.video_configurations.iter().all(|v| v.codec_type == sink.codec_type));
        }
        let cluster = &sdr.services[1].media_sink.as_ref().unwrap().video_configurations[0];
        assert_eq!(
            cluster.codec_resolution,
            Some(VideoCodecResolution::VideoCodecResolution1280x720 as i32)
        );
        assert_eq!(cluster.margin_height, Some(0));
        assert_eq!(cluster.margin_width, Some(0));
        assert_eq!(cluster.frame_rate, Some(VideoFrameRate::VideoFrameRate60 as i32));
        let tiered =
            AaConfig { cluster_tier_width: Some(800), cluster_tier_height: Some(480), ..cfg };
        let sdr = decoded(&tiered);
        let cluster = &sdr.services[1].media_sink.as_ref().unwrap().video_configurations[0];
        assert_eq!(
            cluster.codec_resolution,
            Some(VideoCodecResolution::VideoCodecResolution800x480 as i32)
        );
        assert_eq!(cluster.margin_height, Some(80));
        assert_eq!(VideoCodec::of_media_codec(MediaCodecType::VideoAv1 as i32), VideoCodec::Av1);
        assert_eq!(VideoCodec::of_media_codec(0), VideoCodec::H264);
        assert_eq!(resolution(3840), VideoCodecResolution::VideoCodecResolution3840x2160);
        assert_eq!(resolution(2560), VideoCodecResolution::VideoCodecResolution2560x1440);
    }
}
