// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! One CarPlay audio stream from its RTP packets to the sink.

pub mod uplink;

use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;
use gstreamer_controller as gst_controller;
use gstreamer_controller::prelude::*;
use livi_output_proto::audio::audio_sink;

use crate::stream::{Codec, RTP_HEADER_LEN, aac_raw_caps, alac_caps, reframe_aac, rtp_caps};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};

pub struct Config {
    pub codec: Codec,
    pub payload_type: u8,
    pub clock_rate: u32,
    pub channels: u8,
    pub latency_ms: u32,
    pub realtime: bool,
    pub paced: bool,
    pub device: Option<String>,
    pub label: String,
    pub codec_data: Vec<u8>,
}

fn decoder_chain(codec: Codec) -> Option<&'static str> {
    match codec {
        Codec::Pcm | Codec::PcmLe | Codec::Pcm24 => None,
        Codec::Opus => Some("rtpjitterbuffer latency={latency} ! rtpopusdepay ! opusdec"),
        Codec::AacRaw => Some("aacparse ! faad"),
        Codec::Alac | Codec::Alac24 => Some("avdec_alac"),
        Codec::AacEld => Some(
            "rtpjitterbuffer latency={latency} ! rtpmp4gdepay ! \
             audio/mpeg,rate={rate},channels=2 ! avdec_aac",
        ),
        #[cfg(target_os = "linux")]
        Codec::AacLc => Some("rtpjitterbuffer latency={latency} ! rtpmp4gdepay ! aacparse ! faad"),
        #[cfg(not(target_os = "linux"))]
        Codec::AacLc => {
            Some("rtpjitterbuffer latency={latency} ! rtpmp4gdepay ! aacparse ! avdec_aac")
        }
    }
}

const JITTER_MIN_MS: u32 = 100;

fn jitter_ms(latency_ms: u32) -> u32 {
    latency_ms.max(JITTER_MIN_MS)
}

fn target_ms(cfg: &Config) -> Option<u32> {
    if cfg.realtime || cfg.paced {
        return Some(cfg.latency_ms);
    }
    decoder_chain(cfg.codec)
        .is_some_and(|chain| chain.starts_with("rtpjitterbuffer"))
        .then(|| jitter_ms(cfg.latency_ms))
}

const SINK_NAME: &str = "audio-sink";
const FRAME_BYTES: usize = 4;
const OUT_RATE: u64 = 48_000;
const SERVO_START_EXTRA_MS: f64 = 250.0;
const SERVO_BAND_MS: f64 = 5.0;
const SERVO_WEIGHT: f64 = 0.05;
const SERVO_SECS: f64 = 5.0;
const SERVO_MAX_RATE: f64 = 0.01;
const SERVO_SLEW_PER_S: f64 = 0.005;

struct Servo {
    target_ms: f64,
    level_ms: Option<f64>,
    rate: f64,
    carry: f64,
}

impl Servo {
    fn new(target_ms: f64) -> Self {
        Self {
            target_ms,
            level_ms: None,
            rate: 0.0,
            carry: 0.0,
        }
    }

    fn step(&mut self, delay: std::time::Duration, frames: usize) -> i32 {
        let now = delay.as_secs_f64() * 1000.0;
        let level = self.level_ms.map_or(now, |l| l + SERVO_WEIGHT * (now - l));
        self.level_ms = Some(level);
        let error = self.target_ms - level;
        let wanted = if error.abs() <= SERVO_BAND_MS {
            0.0
        } else {
            (error / (SERVO_SECS * 1000.0)).clamp(-SERVO_MAX_RATE, SERVO_MAX_RATE)
        };
        let slew = SERVO_SLEW_PER_S * frames as f64 / OUT_RATE as f64;
        self.rate += (wanted - self.rate).clamp(-slew, slew);
        self.carry += self.rate * frames as f64;
        let whole = self.carry.trunc();
        self.carry -= whole;
        whole as i32
    }
}

fn nudged_frames(buffer: &gst::Buffer, step: i32) -> Option<Vec<u8>> {
    let map = buffer.map_readable().ok()?;
    let data = map.as_slice();
    let frames = data.len() / FRAME_BYTES;
    let out_frames = usize::try_from(frames as i64 + i64::from(step)).ok()?;
    if frames < 2 || out_frames < 2 {
        return None;
    }
    let at = |frame: usize, ch: usize| {
        let i = frame * FRAME_BYTES + ch * 2;
        f64::from(i16::from_le_bytes([data[i], data[i + 1]]))
    };
    let mut out = Vec::with_capacity(out_frames * FRAME_BYTES);
    for j in 0..out_frames {
        let x = j as f64 * frames as f64 / out_frames as f64;
        let i = (x as usize).min(frames - 1);
        let next = (i + 1).min(frames - 1);
        let t = x - i as f64;
        for ch in 0..2 {
            let v = at(i, ch) + t * (at(next, ch) - at(i, ch));
            out.extend_from_slice(&(v.round() as i16).to_le_bytes());
        }
    }
    Some(out)
}

fn nudge(buffer: gst::Buffer, step: i32) -> gst::Buffer {
    let Some(nudged) = nudged_frames(&buffer, step) else {
        return buffer;
    };
    let frames = (nudged.len() / FRAME_BYTES) as u64;
    let mut out = gst::Buffer::from_mut_slice(nudged);
    if let Some(b) = out.get_mut() {
        b.set_pts(buffer.pts());
        b.set_duration(gst::ClockTime::from_nseconds(
            frames * 1_000_000_000 / OUT_RATE,
        ));
    }
    out
}

fn sink_chain(cfg: &Config) -> String {
    let (element, _device_property) = audio_sink();
    if cfg.realtime || cfg.paced {
        format!("{element} name={SINK_NAME} sync=false buffer-time=40000 latency-time=10000")
    } else {
        format!("{element} name={SINK_NAME} sync=false buffer-time=300000 latency-time=30000")
    }
}

pub fn pipeline_desc(cfg: &Config) -> String {
    let head = match decoder_chain(cfg.codec) {
        Some(chain) => {
            format!(
                "{} ! ",
                chain
                    .replace("{latency}", &jitter_ms(cfg.latency_ms).to_string())
                    .replace("{rate}", &cfg.clock_rate.to_string())
            )
        }
        None => String::new(),
    };

    format!(
        "appsrc name=src is-live=true format=time do-timestamp={stamp} ! \
         {head}audioconvert name=decoded ! clocksync sync={sync} ! volume name=vol ! \
         audioconvert ! audioresample ! audio/x-raw,format=S16LE,rate=48000,channels=2 ! \
         queue name=out ! {sink}",
        stamp = cfg.codec != Codec::AacRaw,
        sync = !cfg.paced,
        sink = sink_chain(cfg),
    )
}

struct UnitClock {
    next: Option<gst::ClockTime>,
    frame: gst::ClockTime,
    headroom: gst::ClockTime,
}

const UNIT_HEADROOM: gst::ClockTime = gst::ClockTime::from_mseconds(250);

fn unit_duration(asc: &[u8]) -> gst::ClockTime {
    const RATES: [u64; 13] = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ];
    let (Some(&a), Some(&b)) = (asc.first(), asc.get(1)) else {
        return gst::ClockTime::from_mseconds(40);
    };
    let index = (((a & 0x07) << 1) | (b >> 7)) as usize;
    let rate = RATES.get(index).copied().unwrap_or(48000);
    let samples = if b & 0x04 != 0 { 960 } else { 1024 };
    gst::ClockTime::from_nseconds(samples * 1_000_000_000 / rate)
}

#[derive(Default)]
struct VizAcc {
    samples: Vec<i16>,
    rate: u32,
}

pub struct Player {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    volume: gst::Element,
    level: gst_controller::InterpolationControlSource,
    visualizer_enabled: Arc<AtomicBool>,
    visualizer: Arc<Mutex<VizAcc>>,
    codec: Codec,
    payload_type: u8,
    units: Mutex<UnitClock>,
    newest: Arc<AtomicU64>,
    played: Arc<AtomicU64>,
    target_ms: Option<u32>,
}

const UNSET: u64 = u64::MAX;

impl Player {
    pub fn new(cfg: &Config) -> Option<Self> {
        ensure_init();

        let desc = pipeline_desc(cfg);
        let pipeline = match gst::parse::launch(&desc) {
            Ok(p) => p.downcast::<gst::Pipeline>().ok()?,
            Err(e) => {
                eprintln!("[cp_audio:{}] pipeline failed: {e}", cfg.label);
                return None;
            }
        };

        if let Some(device) = &cfg.device {
            let (_, device_property) = audio_sink();
            let sink = pipeline.by_name(SINK_NAME)?;
            sink.set_property(device_property, device.as_str());
        }

        let appsrc = pipeline
            .by_name("src")?
            .downcast::<gst_app::AppSrc>()
            .ok()?;
        let caps = match cfg.codec {
            Codec::AacRaw => aac_raw_caps(&cfg.codec_data, cfg.clock_rate, cfg.channels),
            Codec::Alac => alac_caps(cfg.clock_rate, cfg.channels, 16),
            Codec::Alac24 => alac_caps(cfg.clock_rate, cfg.channels, 24),
            _ => rtp_caps(cfg.codec, cfg.payload_type, cfg.clock_rate, cfg.channels),
        };
        let caps = gst::Caps::from_str(&caps).ok()?;
        appsrc.set_caps(Some(&caps));

        let volume = pipeline.by_name("vol")?;
        let level = gst_controller::InterpolationControlSource::new();
        level.set_mode(gst_controller::InterpolationMode::Linear);
        let binding = gst_controller::DirectControlBinding::new_absolute(&volume, "volume", &level);
        volume.add_control_binding(&binding).ok()?;
        volume.set_control_binding_disabled("volume", true);

        let visualizer_enabled = Arc::new(AtomicBool::new(false));
        let visualizer: Arc<Mutex<VizAcc>> = Default::default();
        if let Some(sink_pad) = volume.static_pad("sink") {
            let enabled = visualizer_enabled.clone();
            let acc = visualizer.clone();
            sink_pad.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
                if enabled.load(Ordering::Relaxed)
                    && let Some(buffer) = info.buffer()
                    && let Some(caps) = pad.current_caps()
                    && let Ok(ainfo) = gst_audio::AudioInfo::from_caps(&caps)
                    && let Ok(map) = buffer.map_readable()
                {
                    let ch = ainfo.channels().max(1) as usize;
                    let mono: Vec<i16> = match ainfo.format() {
                        gst_audio::AudioFormat::S16le => map
                            .as_slice()
                            .as_chunks::<2>()
                            .0
                            .chunks(ch)
                            .map(|f| {
                                (f.iter().map(|b| i16::from_le_bytes(*b) as i32).sum::<i32>()
                                    / ch as i32) as i16
                            })
                            .collect(),
                        gst_audio::AudioFormat::F32le => map
                            .as_slice()
                            .as_chunks::<4>()
                            .0
                            .chunks(ch)
                            .map(|f| {
                                let avg = f.iter().map(|b| f32::from_le_bytes(*b)).sum::<f32>()
                                    / ch as f32;
                                (avg.clamp(-1.0, 1.0) * 32767.0) as i16
                            })
                            .collect(),
                        _ => return gst::PadProbeReturn::Ok,
                    };
                    if let Ok(mut a) = acc.lock() {
                        a.rate = ainfo.rate();
                        a.samples.extend_from_slice(&mono);
                        let cap = a.rate as usize / 5;
                        if cap > 0 && a.samples.len() > cap {
                            let overflow = a.samples.len() - cap;
                            a.samples.drain(..overflow);
                        }
                    }
                }
                gst::PadProbeReturn::Ok
            });
        }

        let newest = Arc::new(AtomicU64::new(0));
        if (cfg.realtime || cfg.paced)
            && let Some(queue) = pipeline.by_name("out")
            && let Some(pad) = queue.static_pad("src")
        {
            let target_ms = f64::from(cfg.latency_ms);
            if cfg.realtime {
                let start_ms = target_ms + SERVO_START_EXTRA_MS;
                queue.set_property("min-threshold-time", (start_ms * 1e6) as u64);
            }
            let primed = AtomicBool::new(false);
            let held = queue.downgrade();
            let weak = pipeline.downgrade();
            let servo = Mutex::new(Servo::new(target_ms));
            let paced = cfg.paced.then(|| newest.clone());
            pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                if !primed.swap(true, Ordering::Relaxed)
                    && let Some(queue) = held.upgrade()
                {
                    queue.set_property("min-threshold-time", 0u64);
                }
                let Some(pipeline) = weak.upgrade() else {
                    return gst::PadProbeReturn::Ok;
                };
                let delay = match &paced {
                    Some(newest) => {
                        let pts = info
                            .buffer()
                            .and_then(|b| b.pts())
                            .map_or(UNSET, |p| p.nseconds());
                        ahead(&pipeline, newest.load(Ordering::Relaxed), pts)
                    }
                    None => output_delay(&pipeline),
                };
                let Some(delay) = delay else {
                    return gst::PadProbeReturn::Ok;
                };
                let frames = match &info.data {
                    Some(gst::PadProbeData::Buffer(b)) => b.size() / FRAME_BYTES,
                    _ => 0,
                };
                let step = servo.lock().map_or(0, |mut s| s.step(delay, frames));
                if step != 0
                    && let Some(gst::PadProbeData::Buffer(buffer)) = info.data.take()
                {
                    info.data = Some(gst::PadProbeData::Buffer(nudge(buffer, step)));
                }
                gst::PadProbeReturn::Ok
            });
        }

        let played = Arc::new(AtomicU64::new(UNSET));
        if let Some(pad) = pipeline.by_name("out").and_then(|q| q.static_pad("src")) {
            let played = played.clone();
            pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                if let Some(pts) = info.buffer().and_then(|b| b.pts()) {
                    played.store(pts.nseconds(), Ordering::Relaxed);
                }
                gst::PadProbeReturn::Ok
            });
        }

        if let Some(bus) = pipeline.bus() {
            let label = cfg.label.clone();
            bus.set_sync_handler(move |_, msg| {
                if let gst::MessageView::StreamStatus(s) = msg.view()
                    && s.get().0 == gst::StreamStatusType::Enter
                {
                    crate::realtime::promote(&label);
                }
                if let gst::MessageView::Error(e) = msg.view() {
                    eprintln!(
                        "[cp_audio:{label}] {} | {}",
                        e.error(),
                        e.debug().unwrap_or_default()
                    );
                }
                gst::BusSyncReply::Drop
            });
        }

        Some(Self {
            pipeline,
            appsrc,
            volume,
            level,
            visualizer_enabled,
            visualizer,
            codec: cfg.codec,
            payload_type: cfg.payload_type,
            units: Mutex::new(UnitClock {
                next: None,
                frame: unit_duration(&cfg.codec_data),
                headroom: if cfg.paced {
                    gst::ClockTime::ZERO
                } else {
                    UNIT_HEADROOM
                },
            }),
            newest,
            played,
            target_ms: target_ms(cfg),
        })
    }

    pub fn set_visualizer_enabled(&self, on: bool) {
        self.visualizer_enabled.store(on, Ordering::Relaxed);
        if !on && let Ok(mut a) = self.visualizer.lock() {
            a.samples.clear();
        }
    }

    pub fn take_visualizer(&self) -> Option<(Vec<u8>, u32)> {
        let mut a = self.visualizer.lock().ok()?;
        if a.samples.is_empty() {
            return None;
        }
        let bytes = core::mem::take(&mut a.samples)
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        Some((bytes, a.rate))
    }

    pub fn start(&self) {
        let _ = self.pipeline.set_state(gst::State::Playing);
    }

    pub fn stop(&self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }

    pub fn push_rtp(&self, rtp: &[u8]) -> bool {
        self.handed_in(None);
        let packet = match self.codec {
            Codec::AacLc | Codec::AacEld => reframe_aac(rtp, self.payload_type),
            Codec::Opus | Codec::AacRaw => rtp.to_vec(),
            Codec::Pcm | Codec::PcmLe | Codec::Pcm24 | Codec::Alac | Codec::Alac24 => {
                rtp[RTP_HEADER_LEN.min(rtp.len())..].to_vec()
            }
        };
        self.appsrc
            .push_buffer(gst::Buffer::from_slice(packet))
            .is_ok()
    }

    pub fn push_samples(&self, samples: &[u8]) -> bool {
        let mut buffer = gst::Buffer::from_slice(samples.to_vec());
        if self.codec == Codec::AacRaw
            && let Ok(mut clock) = self.units.lock()
            && let Some(b) = buffer.get_mut()
        {
            let now = self
                .pipeline
                .current_running_time()
                .unwrap_or(gst::ClockTime::ZERO);
            let pts = match clock.next {
                Some(next) if next >= now => next,
                _ => now + clock.headroom,
            };
            b.set_pts(pts);
            b.set_duration(clock.frame);
            clock.next = Some(pts + clock.frame);
            self.handed_in(clock.next);
        } else {
            self.handed_in(None);
        }
        self.appsrc.push_buffer(buffer).is_ok()
    }

    fn handed_in(&self, end: Option<gst::ClockTime>) {
        if let Some(end) = end.or_else(|| self.pipeline.current_running_time()) {
            self.newest.fetch_max(end.nseconds(), Ordering::Relaxed);
        }
    }

    pub fn set_volume(&self, level: f64, ms: u64) {
        let target = level.clamp(0.0, 10.0);
        let now = self.pipeline.current_running_time();

        if ms == 0 || now.is_none() {
            self.volume.set_control_binding_disabled("volume", true);
            self.volume.set_property("volume", target);
            return;
        }

        let now = now.unwrap();
        let from = self.volume.property::<f64>("volume");
        self.level.unset_all();
        self.level.set(now, from);
        self.level
            .set(now + gst::ClockTime::from_mseconds(ms), target);
        self.volume.set_control_binding_disabled("volume", false);
    }

    pub fn target_ms(&self) -> Option<u32> {
        self.target_ms
    }

    pub fn output_delay(&self) -> Option<std::time::Duration> {
        output_delay(&self.pipeline)
    }

    pub fn ahead(&self) -> Option<std::time::Duration> {
        ahead(
            &self.pipeline,
            self.newest.load(Ordering::Relaxed),
            self.played.load(Ordering::Relaxed),
        )
    }

    pub fn decoded(&self) -> Option<(u32, u8)> {
        let caps = self
            .pipeline
            .by_name("decoded")?
            .static_pad("sink")?
            .current_caps()?;
        let info = gst_audio::AudioInfo::from_caps(&caps).ok()?;
        Some((info.rate(), u8::try_from(info.channels()).ok()?))
    }
}

fn ahead(pipeline: &gst::Pipeline, newest: u64, played: u64) -> Option<std::time::Duration> {
    if newest == 0 || played == UNSET {
        return None;
    }
    let ring = pipeline
        .by_name(SINK_NAME)
        .and_then(|s| ring_ns(&s))
        .unwrap_or(0);
    Some(std::time::Duration::from_nanos(
        newest.saturating_sub(played) + ring,
    ))
}

fn output_delay(pipeline: &gst::Pipeline) -> Option<std::time::Duration> {
    let sink = pipeline.by_name(SINK_NAME)?;
    let queued = pipeline
        .by_name("out")
        .map_or(0, |q| u64::from(q.property::<u32>("current-level-bytes")));
    let queued_ns = queued * 1_000_000_000 / (OUT_RATE * FRAME_BYTES as u64);
    Some(std::time::Duration::from_nanos(queued_ns + ring_ns(&sink)?))
}

fn ring_ns(sink: &gst::Element) -> Option<u64> {
    if !sink.is::<gst_audio::AudioBaseSink>() {
        return None;
    }
    // SAFETY: the element is a GstAudioBaseSink, its ring buffer lives as long as the sink.
    let ring_ns = unsafe {
        let base = sink.as_ptr() as *mut gst_audio::ffi::GstAudioBaseSink;
        let ring = (*base).ringbuffer;
        let next = (*base).next_sample;
        if ring.is_null()
            || next == u64::MAX
            || gst_audio::ffi::gst_audio_ring_buffer_is_acquired(ring) == 0
        {
            return None;
        }
        let rate = u64::try_from((*ring).spec.info.rate)
            .ok()
            .filter(|r| *r > 0)?;
        next.checked_sub(gst_audio::ffi::gst_audio_ring_buffer_samples_done(ring))? * 1_000_000_000
            / rate
    };
    Some(ring_ns)
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

pub fn ensure_init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = gst::init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(codec: Codec, realtime: bool) -> Config {
        Config {
            codec,
            payload_type: 96,
            clock_rate: 44100,
            channels: 2,
            latency_ms: 1000,
            realtime,
            paced: false,
            device: None,
            label: "music".into(),
            codec_data: Vec::new(),
        }
    }

    fn elements(desc: &str) -> Vec<String> {
        desc.split('!')
            .map(|p| p.split_whitespace().next().unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn the_aac_chain_runs_from_appsrc_to_the_sink() {
        let desc = pipeline_desc(&cfg(Codec::AacLc, false));

        assert_eq!(
            elements(&desc),
            vec![
                "appsrc",
                "rtpjitterbuffer",
                "rtpmp4gdepay",
                "aacparse",
                if cfg!(target_os = "linux") {
                    "faad"
                } else {
                    "avdec_aac"
                },
                "audioconvert",
                "clocksync",
                "volume",
                "audioconvert",
                "audioresample",
                "audio/x-raw,format=S16LE,rate=48000,channels=2",
                "queue",
                audio_sink().0,
            ]
        );
    }

    #[test]
    fn the_sink_is_fed_the_same_way_on_both_systems() {
        let buffered = sink_chain(&cfg(Codec::AacLc, false));
        assert!(buffered.contains("sync=false"), "{buffered}");
        assert!(buffered.contains("buffer-time=300000"), "{buffered}");
        assert!(buffered.contains("latency-time=30000"), "{buffered}");
        let realtime = sink_chain(&cfg(Codec::Opus, true));
        assert!(realtime.contains("sync=false"), "{realtime}");
        assert!(realtime.contains("buffer-time=40000"), "{realtime}");
    }

    #[test]
    fn opus_depays_and_decodes_as_opus() {
        let desc = pipeline_desc(&cfg(Codec::Opus, false));
        let e = elements(&desc);

        assert!(e.contains(&"rtpopusdepay".to_string()));
        assert!(e.contains(&"opusdec".to_string()));
    }

    #[test]
    fn lpcm_goes_straight_from_the_source_to_the_converter() {
        let desc = pipeline_desc(&cfg(Codec::Pcm, false));
        let e = elements(&desc);

        assert_eq!(e[0], "appsrc");
        assert_eq!(e[1], "audioconvert");
        assert!(!desc.contains("rtpjitterbuffer"));
        assert!(!desc.contains("depay"));
    }

    #[test]
    fn the_jitter_buffer_takes_the_negotiated_depth() {
        let desc = pipeline_desc(&cfg(Codec::AacLc, false));

        assert!(desc.contains("rtpjitterbuffer latency=1000"));
    }

    #[test]
    fn a_depth_below_the_floor_is_raised() {
        let mut c = cfg(Codec::Opus, false);
        c.latency_ms = 10;

        assert!(pipeline_desc(&c).contains("rtpjitterbuffer latency=100"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_paced_stream_is_not_held_back_again() {
        let mut paced = cfg(Codec::AacRaw, false);
        paced.paced = true;
        let desc = pipeline_desc(&paced);
        assert!(desc.contains("clocksync sync=false"), "{desc}");
        assert!(desc.contains("buffer-time=40000"), "{desc}");
        let music = pipeline_desc(&cfg(Codec::AacRaw, false));
        assert!(music.contains("clocksync sync=true"), "{music}");
    }

    #[test]
    fn the_target_is_what_the_stream_is_held_at() {
        assert_eq!(target_ms(&cfg(Codec::AacLc, false)), Some(1000));
        let mut siri = cfg(Codec::Opus, false);
        siri.latency_ms = 80;
        assert_eq!(target_ms(&siri), Some(JITTER_MIN_MS));
        let mut paced = cfg(Codec::AacRaw, false);
        assert_eq!(target_ms(&paced), None);
        paced.paced = true;
        paced.latency_ms = 200;
        assert_eq!(target_ms(&paced), Some(200));
        assert_eq!(target_ms(&cfg(Codec::Pcm, false)), None);
    }

    #[test]
    fn a_realtime_sink_gets_a_short_ring_and_no_audio_is_thrown_away() {
        let music = pipeline_desc(&cfg(Codec::AacLc, false));
        assert!(music.contains("buffer-time=300000"));
        let realtime = pipeline_desc(&cfg(Codec::AacLc, true));
        assert!(realtime.contains("buffer-time=40000"));
        assert!(!realtime.contains("leaky"));
    }

    #[test]
    fn the_servo_glides_toward_the_target() {
        let ms = std::time::Duration::from_millis;
        let mut servo = Servo::new(50.0);
        assert_eq!(servo.step(ms(52), 480), 0);
        assert_eq!(servo.rate, 0.0);

        let mut servo = Servo::new(50.0);
        servo.step(ms(600), 480);
        assert!(
            servo.rate < 0.0 && servo.rate >= -0.000_05,
            "one buffer moves the speed only a little: {}",
            servo.rate
        );
        let dropped: i32 = (0..300).map(|_| servo.step(ms(600), 480)).sum();
        assert_eq!(servo.rate, -SERVO_MAX_RATE, "capped at 1 %");
        assert!((-1100..-900).contains(&dropped), "{dropped}");

        let mut servo = Servo::new(50.0);
        let added: i32 = (0..300).map(|_| servo.step(ms(20), 480)).sum();
        assert!(added > 0, "below the target it slows down");

        let mut servo = Servo::new(50.0);
        servo.step(ms(50), 480);
        servo.step(ms(100), 480);
        assert_eq!(
            servo.rate, 0.0,
            "one outlier only moves the average a little"
        );
    }

    #[test]
    fn a_nudge_stretches_the_buffer_evenly() {
        ensure_init();
        let frame = |l: i16, r: i16| [l.to_le_bytes(), r.to_le_bytes()].concat();
        let input = [
            frame(0, 0),
            frame(100, -100),
            frame(200, -200),
            frame(300, -300),
        ]
        .concat();
        let bytes = |b: gst::Buffer| b.map_readable().unwrap().as_slice().to_vec();

        let added = bytes(nudge(gst::Buffer::from_slice(input.clone()), 1));
        let expected = [
            frame(0, 0),
            frame(80, -80),
            frame(160, -160),
            frame(240, -240),
            frame(300, -300),
        ];
        assert_eq!(added, expected.concat());
        let dropped = bytes(nudge(gst::Buffer::from_slice(input), -1));
        assert_eq!(
            dropped,
            [frame(0, 0), frame(133, -133), frame(267, -267)].concat()
        );
    }

    #[test]
    fn the_desc_never_carries_the_device_string() {
        let mut c = cfg(Codec::Opus, false);
        c.device =
            Some("AppleUSBAudioEngine:Unknown Manufacturer:USB PnP Audio Device:131200:1".into());

        let desc = pipeline_desc(&c);
        assert!(desc.contains(&format!("name={SINK_NAME}")), "desc = {desc}");
        assert!(!desc.contains("unique-id="), "desc = {desc}");
        assert!(!desc.contains(" device="), "desc = {desc}");
        assert!(
            !desc.contains("USB PnP"),
            "device string leaked into desc: {desc}"
        );
    }

    #[test]
    fn every_link_carries_a_space_on_both_sides() {
        let desc = pipeline_desc(&cfg(Codec::AacLc, false));

        assert_eq!(desc.matches(" ! ").count(), desc.matches('!').count());
    }

    #[test]
    fn dab_units_go_through_faad_with_their_own_timestamps() {
        let desc = pipeline_desc(&cfg(Codec::AacRaw, false));
        assert!(desc.contains("do-timestamp=false"));
        assert_eq!(&elements(&desc)[..3], ["appsrc", "aacparse", "faad"]);
        assert!(pipeline_desc(&cfg(Codec::Opus, false)).contains("do-timestamp=true"));
    }

    #[test]
    fn unit_duration_follows_the_core_rate_and_frame_length() {
        assert_eq!(
            unit_duration(&[0x13, 0x14, 0x56, 0xe5, 0x98]).mseconds(),
            40
        );
        assert_eq!(unit_duration(&[0x11, 0x94]).mseconds(), 20);
        assert_eq!(unit_duration(&[]).mseconds(), 40);
    }
}
