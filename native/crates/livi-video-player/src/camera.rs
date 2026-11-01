// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use livi_host_proto::{CameraOrder, CameraSeen};

use crate::Say;

const SOURCE_CLASS: &str = "Video/Source";
const RELIST_EVERY: Duration = Duration::from_secs(3);
const ID_KEYS: [&str; 4] = [
    "device.path",
    "api.v4l2.path",
    "avf.unique_id",
    "object.path",
];
const PROCESSING_DRIVERS: [&str; 4] = ["bcm2835-isp", "bcm2835-codec", "rpi-hevc-dec", "pispbe"];
const CAP_M2M_MPLANE: u32 = 0x4000;
const CAP_M2M: u32 = 0x8000;
const JPEG_DECODERS: [&str; 3] = ["jpegdec", "avdec_mjpeg", "vtdec"];
const FLUID_FPS: f64 = 25.0;
const AIM_HEIGHT: i32 = 720;
const TOP_HEIGHT: i32 = 1080;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Raw,
    Jpeg,
    H264,
}

impl Kind {
    fn caps_name(self) -> &'static str {
        match self {
            Kind::Raw => "video/x-raw",
            Kind::Jpeg => "image/jpeg",
            Kind::H264 => "video/x-h264",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Mode {
    kind: Kind,
    width: i32,
    height: i32,
    fps: f64,
}

impl Mode {
    fn fluid(&self) -> bool {
        self.fps >= FLUID_FPS
    }

    fn size_rank(&self) -> i32 {
        match self.height {
            h if (AIM_HEIGHT..=TOP_HEIGHT).contains(&h) => h - AIM_HEIGHT,
            h if h > TOP_HEIGHT => 10_000 + h - TOP_HEIGHT,
            h => 20_000 + AIM_HEIGHT - h,
        }
    }

    fn caps(&self) -> String {
        let rate = if self.fluid() {
            ",framerate=[25/1,1000/1]"
        } else {
            ""
        };
        format!(
            "{},width={},height={}{rate}",
            self.kind.caps_name(),
            self.width,
            self.height
        )
    }
}

fn pick(modes: &[Mode], jpeg: bool, h264: bool) -> Option<Mode> {
    modes
        .iter()
        .filter(|m| match m.kind {
            Kind::Raw => true,
            Kind::Jpeg => jpeg,
            Kind::H264 => h264,
        })
        .min_by(|a, b| {
            b.fluid()
                .cmp(&a.fluid())
                .then(a.size_rank().cmp(&b.size_rank()))
                .then(a.kind.cmp(&b.kind))
                .then(b.fps.total_cmp(&a.fps))
        })
        .copied()
}

fn top_fps(s: &gst::StructureRef) -> f64 {
    let f = |r: gst::Fraction| f64::from(r.numer()) / f64::from(r.denom().max(1));
    if let Ok(r) = s.get::<gst::Fraction>("framerate") {
        return f(r);
    }
    if let Ok(r) = s.get::<gst::FractionRange>("framerate") {
        return f(r.max());
    }
    s.get::<gst::List>("framerate")
        .map(|l| {
            l.iter()
                .filter_map(|v| v.get::<gst::Fraction>().ok())
                .map(f)
                .fold(0.0, f64::max)
        })
        .unwrap_or(0.0)
}

fn side(s: &gst::StructureRef, name: &str, aim: i32) -> Option<i32> {
    s.get::<i32>(name).ok().or_else(|| {
        s.get::<gst::IntRange<i32>>(name)
            .ok()
            .map(|r| aim.clamp(r.min(), r.max()))
    })
}

fn modes(caps: &gst::Caps) -> Vec<Mode> {
    caps.iter()
        .filter_map(|s| {
            let kind = match s.name().as_str() {
                "video/x-raw" => Kind::Raw,
                "image/jpeg" => Kind::Jpeg,
                "video/x-h264" => Kind::H264,
                _ => return None,
            };
            Some(Mode {
                kind,
                width: side(s, "width", 1280)?,
                height: side(s, "height", AIM_HEIGHT)?,
                fps: top_fps(s),
            })
        })
        .collect()
}

fn chain(mode: Option<Mode>, jpeg: Option<&str>, h264: Option<&str>) -> String {
    let (caps, decode) = match (mode, jpeg, h264) {
        (Some(m), Some(dec), _) if m.kind == Kind::Jpeg => (m.caps(), format!(" ! {dec}")),
        (Some(m), _, Some(dec)) if m.kind == Kind::H264 => {
            (m.caps(), format!(" ! h264parse ! {dec}"))
        }
        (Some(m), ..) if m.kind == Kind::Raw => (m.caps(), String::new()),
        _ => ("video/x-raw".to_owned(), String::new()),
    };
    format!("capsfilter caps=\"{caps}\"{decode} ! videoconvert ! capsfilter caps=video/x-raw")
}

fn intake(caps: Option<&gst::Caps>, say: &Say) -> String {
    let jpeg = JPEG_DECODERS.iter().copied().find(|n| crate::usable(n));
    let h264 = crate::candidates("h264", false)
        .into_iter()
        .find(|n| crate::usable(n));
    let mode = caps
        .map(modes)
        .and_then(|m| pick(&m, jpeg.is_some(), h264.is_some()));
    let chain = chain(mode, jpeg, h264.as_deref());
    say(&format!("[camera] {chain}"));
    chain
}

pub fn direction(rotation: u16, mirror: bool) -> &'static str {
    match (rotation % 360, mirror) {
        (90, false) => "90r",
        (180, false) => "180",
        (270, false) => "90l",
        (0, true) => "horiz",
        (90, true) => "ur-ll",
        (180, true) => "vert",
        (270, true) => "ul-lr",
        _ => "identity",
    }
}

fn device_id(device: &gst::Device) -> String {
    device
        .properties()
        .and_then(|p| {
            ID_KEYS
                .iter()
                .find_map(|k| p.get::<String>(*k).ok().filter(|v| !v.is_empty()))
        })
        .unwrap_or_else(|| device.display_name().to_string())
}

fn monitor() -> gst::DeviceMonitor {
    let monitor = gst::DeviceMonitor::new();
    monitor.add_filter(Some(SOURCE_CLASS), None);
    monitor
}

fn is_camera(driver: &str, device_caps: u32) -> bool {
    !PROCESSING_DRIVERS.contains(&driver) && device_caps & (CAP_M2M | CAP_M2M_MPLANE) == 0
}

fn camera_devices(monitor: &gst::DeviceMonitor) -> Vec<gst::Device> {
    monitor
        .devices()
        .into_iter()
        .filter(|d| {
            let props = d.properties();
            let driver = props
                .as_ref()
                .and_then(|p| p.get::<String>("v4l2.device.driver").ok())
                .unwrap_or_default();
            let caps = props
                .as_ref()
                .and_then(|p| p.get::<u32>("v4l2.device.device_caps").ok())
                .unwrap_or(0);
            is_camera(&driver, caps)
        })
        .collect()
}

fn seen(monitor: &gst::DeviceMonitor) -> Vec<CameraSeen> {
    let mut list: Vec<CameraSeen> = camera_devices(monitor)
        .iter()
        .map(|d| CameraSeen {
            id: device_id(d),
            name: d.display_name().to_string(),
        })
        .collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    list.dedup_by(|a, b| a.id == b.id);
    list
}

pub fn watch_cameras(report: impl Fn(Vec<CameraSeen>) + Send + 'static) {
    crate::ensure_init();
    let _ = std::thread::Builder::new()
        .name("cameras".into())
        .spawn(move || {
            let monitor = monitor();
            let started = monitor.start().is_ok();
            let mut last = seen(&monitor);
            report(last.clone());
            let bus = monitor.bus();
            loop {
                if started {
                    let _ = bus.timed_pop(gst::ClockTime::from_mseconds(
                        RELIST_EVERY.as_millis() as u64
                    ));
                } else {
                    std::thread::sleep(RELIST_EVERY);
                }
                let now = seen(&monitor);
                if now != last {
                    report(now.clone());
                    last = now;
                }
            }
        });
}

fn source(device: &str, say: &Say) -> Option<(gst::Element, Option<gst::Caps>)> {
    let monitor = monitor();
    let devices = camera_devices(&monitor);
    let found = devices
        .iter()
        .find(|d| device_id(d) == device)
        .or_else(|| {
            say(&format!(
                "[camera] {device} is gone, taking the first camera"
            ));
            devices.first()
        })?;
    say(&format!("[camera] opening {}", found.display_name()));
    let caps = found.caps();
    found
        .create_element(None)
        .inspect_err(|e| say(&format!("[camera] cannot open {device}: {e}")))
        .ok()
        .map(|element| (element, caps))
}

pub fn gl_turn(turn: &str) -> &'static str {
    match turn {
        "90r" => "clockwise",
        "180" => "rotate-180",
        "90l" => "counterclockwise",
        "horiz" => "horizontal-flip",
        "vert" => "vertical-flip",
        "ul-lr" => "upper-left-diagonal",
        "ur-ll" => "upper-right-diagonal",
        _ => "none",
    }
}

fn rotate_value(element: &gst::Element, turn: &str) -> Option<gst::glib::Value> {
    let spec = element.find_property("rotate-method")?;
    let class = gst::glib::EnumClass::with_type(spec.value_type())?;
    [turn, gl_turn(turn)]
        .into_iter()
        .find_map(|nick| class.to_value_by_nick(nick))
}

fn turn_in_sink(sink: &gst::Element, turn: &str) -> bool {
    let mut all = vec![sink.clone()];
    if let Some(bin) = sink.downcast_ref::<gst::Bin>() {
        all.extend(bin.iterate_recurse().into_iter().flatten());
    }
    all.iter()
        .find_map(|e| rotate_value(e, turn).map(|v| (e, v)))
        .map(|(e, v)| e.set_property_from_value("rotate-method", &v))
        .is_some()
}

pub struct CameraPlayer {
    pipeline: gst::Pipeline,
    view: crate::WindowView,
    say: Say,
}

impl CameraPlayer {
    pub fn new(order: &CameraOrder, window: usize, say: Say) -> Option<Self> {
        crate::ensure_init();
        let (src, caps) = source(&order.device, &say)?;
        let sink = crate::url_player::video_sink(&say)?;
        let turn = direction(order.rotation, order.mirror);
        let turned = turn == "identity" || turn_in_sink(&sink, turn);
        let head = intake(caps.as_ref(), &say);
        let shaping = if turned {
            head
        } else if gst::ElementFactory::find("videoflip").is_some() {
            format!("{head} ! videoflip video-direction={turn}")
        } else {
            say(&format!(
                "[camera] nothing here can turn the picture {turn}"
            ));
            head
        };
        let shape = gst::parse::bin_from_description(&shaping, true)
            .inspect_err(|e| say(&format!("[camera] {shaping}: {e}")))
            .ok()?;
        let pipeline = gst::Pipeline::with_name("camera");
        pipeline
            .add_many([&src, shape.upcast_ref(), &sink])
            .inspect_err(|e| say(&format!("[camera] cannot build: {e}")))
            .ok()?;
        gst::Element::link_many([&src, shape.upcast_ref(), &sink])
            .inspect_err(|e| say(&format!("[camera] cannot link: {e}")))
            .ok()?;
        crate::log_bus_messages(&pipeline);
        crate::force_sinks_realtime(&pipeline);
        for element in pipeline.iterate_recurse().into_iter().flatten() {
            if element.is::<gstreamer_base::BaseSink>() {
                element.set_property("async", false);
            }
        }
        if let Some(pad) = shape.static_pad("src") {
            let first = say.clone();
            pad.add_probe(gst::PadProbeType::BUFFER, move |pad, _| {
                let caps = pad
                    .current_caps()
                    .and_then(|c| crate::decoded_caps(&c))
                    .unwrap_or_default();
                first(&format!("[camera] first picture {caps}"));
                gst::PadProbeReturn::Remove
            });
        }
        let view = crate::WindowView::default();
        view.attach(window, crate::RANK_CAMERA, Some(&sink));
        say(&format!(
            "[camera] rotation {} mirror {}",
            order.rotation, order.mirror
        ));
        Some(Self {
            pipeline,
            view,
            say,
        })
    }

    pub fn start(&self) {
        let asked = self.pipeline.set_state(gst::State::Playing);
        (self.say)(&format!("[camera] start {asked:?}"));
    }

    pub fn stop(&self) {
        (self.say)("[camera] stop");
        let _ = self.pipeline.set_state(gst::State::Null);
        self.view.remove();
    }

    pub fn set_visible(&self, visible: bool) {
        self.view.set_visible(visible);
    }

    pub fn set_content_region(
        &self,
        crop_l: f64,
        crop_t: f64,
        vis_w: f64,
        vis_h: f64,
        tier_w: f64,
        tier_h: f64,
    ) {
        self.view
            .set_region(crop_l, crop_t, vis_w, vis_h, tier_w, tier_h);
    }
}

impl Drop for CameraPlayer {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        self.view.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_after_the_mirror_gives_one_flip() {
        assert_eq!(direction(0, false), "identity");
        assert_eq!(direction(90, false), "90r");
        assert_eq!(direction(180, false), "180");
        assert_eq!(direction(270, false), "90l");
        assert_eq!(direction(0, true), "horiz");
        assert_eq!(direction(90, true), "ur-ll");
        assert_eq!(direction(180, true), "vert");
        assert_eq!(direction(270, true), "ul-lr");
        assert_eq!(direction(45, false), "identity");
    }

    #[test]
    fn every_turn_has_a_gl_name() {
        let turns = [
            "90r", "180", "90l", "horiz", "vert", "ul-lr", "ur-ll", "identity",
        ];
        let names: Vec<&str> = turns.iter().map(|t| gl_turn(t)).collect();
        assert_eq!(
            names,
            [
                "clockwise",
                "rotate-180",
                "counterclockwise",
                "horizontal-flip",
                "vertical-flip",
                "upper-left-diagonal",
                "upper-right-diagonal",
                "none"
            ]
        );
    }

    fn mode(kind: Kind, width: i32, height: i32, fps: f64) -> Mode {
        Mode {
            kind,
            width,
            height,
            fps,
        }
    }

    #[test]
    fn a_fluid_720p_wins_over_small_raw_and_big_pictures() {
        let usb = [
            mode(Kind::Jpeg, 1920, 1080, 30.0),
            mode(Kind::Jpeg, 1280, 720, 30.0),
            mode(Kind::Jpeg, 640, 480, 30.0),
            mode(Kind::Raw, 1920, 1080, 5.0),
            mode(Kind::Raw, 1280, 720, 10.0),
            mode(Kind::Raw, 640, 480, 30.0),
        ];
        assert_eq!(pick(&usb, true, true), Some(usb[1]));
        assert_eq!(pick(&usb, false, false), Some(usb[5]));
        let facetime = [
            mode(Kind::Raw, 1920, 1080, 30.0),
            mode(Kind::Raw, 1280, 720, 30.0),
            mode(Kind::Raw, 640, 480, 30.0),
        ];
        assert_eq!(pick(&facetime, true, true), Some(facetime[1]));
        let h264 = [
            mode(Kind::H264, 1280, 720, 30.0),
            mode(Kind::Jpeg, 1280, 720, 30.0),
        ];
        assert_eq!(pick(&h264, true, true), Some(h264[1]));
        assert_eq!(pick(&h264, false, true), Some(h264[0]));
        assert_eq!(pick(&[], true, true), None);
        assert_eq!(
            usb[1].caps(),
            "image/jpeg,width=1280,height=720,framerate=[25/1,1000/1]"
        );
        assert_eq!(usb[4].caps(), "video/x-raw,width=1280,height=720");
    }

    #[test]
    fn each_format_gets_its_own_decoder_and_no_decodebin() {
        let jpeg = mode(Kind::Jpeg, 1280, 720, 30.0);
        assert_eq!(
            chain(Some(jpeg), Some("jpegdec"), None),
            "capsfilter caps=\"image/jpeg,width=1280,height=720,framerate=[25/1,1000/1]\" ! jpegdec ! videoconvert ! capsfilter caps=video/x-raw"
        );
        let h264 = mode(Kind::H264, 1280, 720, 30.0);
        assert!(
            chain(Some(h264), None, Some("v4l2h264dec")).contains("! h264parse ! v4l2h264dec !")
        );
        let raw = mode(Kind::Raw, 1280, 720, 30.0);
        assert!(
            chain(Some(raw), None, None).starts_with("capsfilter caps=\"video/x-raw,width=1280")
        );
        assert!(!chain(Some(raw), None, None).contains("decodebin"));
        assert_eq!(
            chain(None, None, None),
            "capsfilter caps=\"video/x-raw\" ! videoconvert ! capsfilter caps=video/x-raw"
        );
    }

    #[test]
    fn device_caps_turn_into_modes() {
        crate::ensure_init();
        let caps: gst::Caps =
            "image/jpeg, width=(int)1280, height=(int)720, framerate=(fraction){ 30/1, 15/1 }; \
             video/x-raw, format=(string)YUY2, width=(int)640, height=(int)480, framerate=(fraction)30/1"
                .parse()
                .unwrap();
        let m = modes(&caps);
        assert_eq!(
            m,
            vec![
                mode(Kind::Jpeg, 1280, 720, 30.0),
                mode(Kind::Raw, 640, 480, 30.0)
            ]
        );
    }

    #[test]
    fn processing_blocks_are_no_cameras() {
        assert!(is_camera("uvcvideo", 0x0420_0001));
        assert!(is_camera("unicam", 0x0420_0001));
        assert!(is_camera("", 0));
        assert!(!is_camera("bcm2835-isp", 0x0420_0001));
        assert!(!is_camera("bcm2835-codec", 0x0420_4000));
        assert!(!is_camera("vivid-m2m", 0x0420_8000));
    }
}
