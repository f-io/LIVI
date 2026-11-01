// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use gstreamer as gst;
use gstreamer_app as gst_app;

use crate::uplink::{Counters, RTP_HEADER_LEN, UplinkCodec, seal_packet, to_wire_pcm};
use gst::prelude::*;
use std::net::{IpAddr, SocketAddr, SocketAddrV6, UdpSocket};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct UplinkConfig {
    pub codec: UplinkCodec,
    pub payload_type: u8,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate: u32,
    pub frame_ms: u32,
    pub key: [u8; 32],
    pub device: Option<String>,
    pub phone: String,
    pub port: u16,
    pub label: String,
}

impl UplinkConfig {
    pub fn samples_per_packet(&self) -> u32 {
        (self.sample_rate * self.frame_ms).div_ceil(1000)
    }
}

fn phone_target(phone: &str, port: u16) -> Option<SocketAddr> {
    let (host, interface) = match phone.split_once('%') {
        Some((host, interface)) => (host, interface.parse().ok()?),
        None => (phone, 0),
    };
    match host.parse().ok()? {
        IpAddr::V4(ip) => Some(SocketAddr::from((ip, port))),
        IpAddr::V6(ip) => Some(SocketAddr::V6(SocketAddrV6::new(ip, port, 0, interface))),
    }
}

pub(crate) const SOURCE_NAME: &str = "audio-source";

pub(crate) fn source_device_property() -> &'static str {
    if cfg!(target_os = "macos") {
        "unique-id"
    } else {
        "device"
    }
}

pub fn pipeline_desc(cfg: &UplinkConfig) -> String {
    let source = if cfg!(target_os = "macos") {
        format!("osxaudiosrc name={SOURCE_NAME}")
    } else {
        format!("pulsesrc name={SOURCE_NAME}")
    };

    let rate = cfg.sample_rate;
    let channels = cfg.channels;
    match cfg.codec {
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

        if let Some(device) = &cfg.device {
            let source = pipeline.by_name(SOURCE_NAME)?;
            source.set_property(source_device_property(), device.as_str());
        }

        let sink = pipeline
            .by_name("out")?
            .downcast::<gst_app::AppSink>()
            .ok()?;
        let Some(target) = phone_target(&cfg.phone, cfg.port) else {
            eprintln!(
                "[cp_mic:{}] no address to reach the phone: {}",
                cfg.label, cfg.phone
            );
            return None;
        };
        let socket = UdpSocket::bind(if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .ok()?;
        let label = cfg.label.clone();
        let send_failed = AtomicBool::new(false);
        let send = move |pkt: &[u8]| {
            if let Err(e) = socket.send_to(pkt, target)
                && !send_failed.swap(true, Ordering::Relaxed)
            {
                eprintln!("[cp_mic:{label}] cannot send to {target}: {e}");
            }
        };
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
                                send(&pkt);
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
                                    send(&pkt);
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

pub fn capture_desc(sample_rate: u32, channels: u8) -> String {
    let source = if cfg!(target_os = "macos") {
        format!("osxaudiosrc name={SOURCE_NAME}")
    } else {
        format!("pulsesrc name={SOURCE_NAME}")
    };
    format!(
        "{source} ! audioconvert ! audioresample ! \
         audio/x-raw,format=S16LE,layout=interleaved,rate={sample_rate},channels={channels} ! \
         appsink name=out sync=false"
    )
}

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
        let desc = capture_desc(sample_rate, channels);
        let pipeline = match gst::parse::launch(&desc) {
            Ok(p) => p.downcast::<gst::Pipeline>().ok()?,
            Err(e) => {
                eprintln!("[mic:{label}] pipeline failed: {e}");
                return None;
            }
        };
        if let Some(device) = device {
            let source = pipeline.by_name(SOURCE_NAME)?;
            source.set_property(source_device_property(), device);
        }
        let sink = pipeline
            .by_name("out")?
            .downcast::<gst_app::AppSink>()
            .ok()?;
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

pub struct SocketTap {
    capture: PcmCapture,
    sock: std::os::unix::net::UnixStream,
}

impl SocketTap {
    pub fn stop(&self) {
        self.hang_up();
        self.capture.stop()
    }

    fn hang_up(&self) {
        let _ = self.sock.shutdown(std::net::Shutdown::Both);
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
        let closer = match sock.try_clone() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[mic:{label}] socket clone: {e}");
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
        Some(Self {
            capture,
            sock: closer,
        })
    }
}

impl Drop for SocketTap {
    fn drop(&mut self) {
        self.hang_up();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uplink::UplinkCodec;

    const NASTY: &str = "AppleUSBAudioEngine:Unknown Manufacturer:USB PnP Audio Device:131200:1";

    fn cfg(codec: UplinkCodec, device: Option<String>) -> UplinkConfig {
        UplinkConfig {
            codec,
            payload_type: 0,
            sample_rate: 48_000,
            channels: 1,
            bitrate: 0,
            frame_ms: 20,
            key: [0; 32],
            device,
            phone: "127.0.0.1".into(),
            port: 0,
            label: "test".into(),
        }
    }

    fn desc_carries_no_device(desc: &str) {
        assert!(
            desc.contains(&format!("name={SOURCE_NAME}")),
            "desc = {desc}"
        );
        assert!(!desc.contains("unique-id="), "desc = {desc}");
        assert!(!desc.contains(" device="), "desc = {desc}");
        assert!(
            !desc.contains(NASTY),
            "device string leaked into desc: {desc}"
        );
    }

    #[test]
    fn pipeline_desc_never_carries_the_device_string() {
        desc_carries_no_device(&pipeline_desc(&cfg(UplinkCodec::Pcm, Some(NASTY.into()))));
        desc_carries_no_device(&pipeline_desc(&cfg(UplinkCodec::Opus, Some(NASTY.into()))));
    }

    #[test]
    fn capture_desc_never_carries_the_device_string() {
        desc_carries_no_device(&capture_desc(48_000, 1));
    }

    #[test]
    fn the_phone_is_reached_on_its_interface() {
        let link_local = phone_target("fe80::fe:6721:d43e:f866%41", 56505).unwrap();
        assert_eq!(
            link_local,
            "[fe80::fe:6721:d43e:f866%41]:56505".parse().unwrap()
        );
        assert_eq!(phone_target("10.0.0.3", 1), "10.0.0.3:1".parse().ok());
        assert_eq!(phone_target("fe80::2%wlan0", 1), None);
        assert_eq!(phone_target("phone", 1), None);
    }
}
