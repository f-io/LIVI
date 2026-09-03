//! Capture for the CarPlay microphone stream.
//!
//! One pipeline captures, converts and, for a wireless phone, encodes to Opus.
//! Every buffer that reaches the appsink is sealed and sent to the phone.

use gstreamer as gst;
use gstreamer_app as gst_app;

use gst::prelude::*;
use livi_audio_uplink::{seal_packet, to_wire_pcm, Counters, UplinkCodec, RTP_HEADER_LEN};
use std::net::UdpSocket;
use std::sync::Mutex;

/// What the phone negotiated for its microphone stream.
pub struct UplinkConfig {
    pub codec: UplinkCodec,
    pub payload_type: u8,
    pub sample_rate: u32,
    pub channels: u8,
    /// Opus target bitrate, tiered by sample rate.
    pub bitrate: u32,
    /// Packet duration, which also sets the timestamp step.
    pub frame_ms: u32,
    pub key: [u8; 32],
    pub device: Option<String>,
    /// Where the sealed packets go.
    pub phone: String,
    pub port: u16,
    pub label: String,
}

impl UplinkConfig {
    /// Samples one packet carries.
    pub fn samples_per_packet(&self) -> u32 {
        (self.sample_rate * self.frame_ms).div_ceil(1000)
    }
}

/// The capture chain. Opus is encoded here, PCM leaves as raw samples.
pub fn pipeline_desc(cfg: &UplinkConfig) -> String {
    let mut source = String::from(if cfg!(target_os = "macos") {
        "osxaudiosrc"
    } else {
        "pulsesrc"
    });
    if let Some(device) = &cfg.device {
        let prop = if cfg!(target_os = "macos") { "unique-id" } else { "device" };
        source.push_str(&format!(" {prop}={device}"));
    }

    let rate = cfg.sample_rate;
    let channels = cfg.channels;
    match cfg.codec {
        // rtpopuspay gives the frame CarPlay expects and drops the OpusHead and
        // OpusTags buffers opusenc emits first. The 12-byte RTP header is
        // stripped in the callback.
        UplinkCodec::Opus => format!(
            "{source} ! audioconvert ! audioresample ! \
             audio/x-raw,rate={rate},channels={channels} ! \
             opusenc bitrate={bitrate} frame-size={frame} ! rtpopuspay ! \
             appsink name=out sync=false",
            bitrate = cfg.bitrate,
            frame = cfg.frame_ms,
        ),
        UplinkCodec::Pcm => format!(
            "{source} ! audioconvert ! audioresample ! \
             audio/x-raw,format=S16LE,layout=interleaved,rate={rate},channels={channels} ! \
             appsink name=out sync=false"
        ),
    }
}

pub struct Uplink {
    pipeline: gst::Pipeline,
}

impl Uplink {
    pub fn new(cfg: UplinkConfig) -> Option<Self> {
        super::ensure_init();

        let desc = pipeline_desc(&cfg);
        let pipeline = match gst::parse::launch(&desc) {
            Ok(p) => p.downcast::<gst::Pipeline>().ok()?,
            Err(e) => {
                eprintln!("[cp_mic:{}] pipeline failed: {e}", cfg.label);
                return None;
            }
        };

        let sink = pipeline.by_name("out")?.downcast::<gst_app::AppSink>().ok()?;
        let socket = UdpSocket::bind("[::]:0").ok()?;
        let target = format!("{}:{}", cfg.phone, cfg.port);
        let samples = cfg.samples_per_packet();
        let frame_bytes = (samples * u32::from(cfg.channels) * 2) as usize;
        let state = Mutex::new((Counters::default(), Vec::<u8>::new()));

        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let Ok(sample) = sink.pull_sample() else {
                        return Err(gst::FlowError::Eos);
                    };
                    let Some(buffer) = sample.buffer() else {
                        return Ok(gst::FlowSuccess::Ok);
                    };
                    let Ok(map) = buffer.map_readable() else {
                        return Ok(gst::FlowSuccess::Ok);
                    };
                    let Ok(mut guard) = state.lock() else {
                        return Ok(gst::FlowSuccess::Ok);
                    };
                    let (counters, pending) = &mut *guard;

                    match cfg.codec {
                        UplinkCodec::Opus => {
                            let frame = map.get(RTP_HEADER_LEN..).unwrap_or(&map);
                            if let Some(pkt) =
                                seal_packet(&cfg.key, cfg.payload_type, counters, frame, samples)
                            {
                                let _ = socket.send_to(&pkt, &target);
                            }
                        }
                        UplinkCodec::Pcm => {
                            pending.extend_from_slice(&map);
                            while pending.len() >= frame_bytes {
                                let frame: Vec<u8> = pending.drain(..frame_bytes).collect();
                                if let Some(pkt) = seal_packet(
                                    &cfg.key,
                                    cfg.payload_type,
                                    counters,
                                    &to_wire_pcm(&frame),
                                    samples,
                                ) {
                                    let _ = socket.send_to(&pkt, &target);
                                }
                            }
                        }
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        Some(Self { pipeline })
    }

    pub fn start(&self) {
        let _ = self.pipeline.set_state(gst::State::Playing);
    }
}

impl Drop for Uplink {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// The capture chain alone: raw samples in the given format, to a callback.
pub fn capture_desc(sample_rate: u32, channels: u8, device: Option<&str>) -> String {
    let mut source = String::from(if cfg!(target_os = "macos") { "osxaudiosrc" } else { "pulsesrc" });
    if let Some(device) = device {
        let prop = if cfg!(target_os = "macos") { "unique-id" } else { "device" };
        source.push_str(&format!(" {prop}={device}"));
    }
    format!(
        "{source} ! audioconvert ! audioresample ! \
         audio/x-raw,format=S16LE,layout=interleaved,rate={sample_rate},channels={channels} ! \
         appsink name=out sync=false"
    )
}

/// A microphone tap: every captured buffer goes to the callback as raw S16LE samples.
pub struct PcmCapture {
    pipeline: gst::Pipeline,
}

impl PcmCapture {
    pub fn new(
        sample_rate: u32,
        channels: u8,
        device: Option<&str>,
        label: &str,
        mut on_pcm: impl FnMut(&[u8]) + Send + 'static,
    ) -> Option<Self> {
        super::ensure_init();
        let desc = capture_desc(sample_rate, channels, device);
        let pipeline = match gst::parse::launch(&desc) {
            Ok(p) => p.downcast::<gst::Pipeline>().ok()?,
            Err(e) => {
                eprintln!("[mic:{label}] pipeline failed: {e}");
                return None;
            }
        };
        let sink = pipeline.by_name("out")?.downcast::<gst_app::AppSink>().ok()?;
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let Ok(sample) = sink.pull_sample() else {
                        return Err(gst::FlowError::Eos);
                    };
                    if let Some(buffer) = sample.buffer()
                        && let Ok(map) = buffer.map_readable()
                    {
                        on_pcm(&map);
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        Some(Self { pipeline })
    }

    pub fn start(&self) {
        let _ = self.pipeline.set_state(gst::State::Playing);
    }

    pub fn stop(&self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl Drop for PcmCapture {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// A microphone tap that streams its samples as feed records into a unix socket
/// someone listens on: the helper, for the phone's mic channel or a call.
pub struct SocketTap {
    capture: PcmCapture,
}

impl SocketTap {
    /// Ends the capture, the socket closes with the tap.
    pub fn stop(&self) {
        self.capture.stop()
    }

    pub fn open(
        path: &str,
        sample_rate: u32,
        channels: u8,
        device: Option<&str>,
        label: &str,
    ) -> Option<Self> {
        let mut sock = match std::os::unix::net::UnixStream::connect(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[mic:{label}] cannot reach {path}: {e}");
                return None;
            }
        };
        let tag = label.to_owned();
        let mut broken = false;
        let capture = PcmCapture::new(sample_rate, channels, device, label, move |pcm| {
            if broken {
                return;
            }
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let record = livi_host_proto::feed::encode(livi_host_proto::feed::KIND_MIC, 0, ts, pcm);
            if std::io::Write::write_all(&mut sock, &record).is_err() {
                eprintln!("[mic:{tag}] the listener is gone");
                broken = true;
            }
        })?;
        capture.start();
        Some(Self { capture })
    }
}
