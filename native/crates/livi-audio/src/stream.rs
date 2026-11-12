// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_crypto::{ChaCha, nonce64};
pub use livi_output_proto::audio::AudioCodec as Codec;

pub const RTP_HEADER_LEN: usize = 12;
pub const TAIL_LEN: usize = 24;
const NONCE_LEN: usize = 8;

pub trait AudioSink {
    fn on_started(&mut self, first_sample: u32);
    fn on_rtp(&mut self, rtp: &[u8], sample: u32);
}

pub struct AudioStream {
    key: ChaCha,
    started: bool,
    sink: Box<dyn AudioSink + Send>,
}

impl AudioStream {
    pub fn new(key: [u8; 32], sink: Box<dyn AudioSink + Send>) -> Self {
        Self {
            key: ChaCha::new(&key),
            started: false,
            sink,
        }
    }

    pub fn reset(&mut self) {
        self.started = false;
    }

    pub fn push(&mut self, packet: &[u8]) {
        if packet.len() < RTP_HEADER_LEN + TAIL_LEN {
            return;
        }
        let end = packet.len();
        let aad = &packet[4..RTP_HEADER_LEN];
        let body = &packet[RTP_HEADER_LEN..end - TAIL_LEN];
        let tag = &packet[end - TAIL_LEN..end - NONCE_LEN];
        let counter = packet[end - NONCE_LEN..].try_into().map(u64::from_le_bytes);
        let Ok(counter) = counter else {
            return;
        };

        let Some(payload) = self.key.open_detached(&nonce64(counter), body, tag, aad) else {
            eprintln!("[cp_audio] packet failed authentication");
            return;
        };

        let sample = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
        if !self.started {
            self.started = true;
            self.sink.on_started(sample);
        }

        let mut rtp = Vec::with_capacity(RTP_HEADER_LEN + payload.len());
        rtp.extend_from_slice(&packet[..RTP_HEADER_LEN]);
        rtp.extend_from_slice(&payload);
        self.sink.on_rtp(&rtp, sample);
    }
}

fn aac_freq_index(clock_rate: u32) -> u16 {
    match clock_rate {
        96000 => 0,
        88200 => 1,
        64000 => 2,
        48000 => 3,
        44100 => 4,
        32000 => 5,
        24000 => 6,
        22050 => 7,
        16000 => 8,
        _ => 3,
    }
}

pub fn lpcm_caps(codec: Codec, clock_rate: u32, channels: u8) -> String {
    let format = if codec == Codec::PcmLe {
        "S16LE"
    } else {
        "S16BE"
    };
    format!("audio/x-raw,format={format},layout=interleaved,rate={clock_rate},channels={channels}")
}

const AOT_ESCAPE: u32 = 31;
const AOT_ER_AAC_ELD: u32 = 39;
const ALAC_FRAMES: u32 = 352;

fn eld_config(clock_rate: u32, channels: u8) -> String {
    let frame_length_480 = 1;
    let config = (AOT_ESCAPE << 27)
        | ((AOT_ER_AAC_ELD - 32) << 21)
        | (u32::from(aac_freq_index(clock_rate)) << 17)
        | (u32::from(channels) << 13)
        | (frame_length_480 << 12);
    format!("{config:08x}")
}

fn alac_cookie(clock_rate: u32, channels: u8, bits: u8) -> String {
    format!(
        "00000024616c616300000000{ALAC_FRAMES:08x}00{bits:02x}280a0e{channels:02x}00ff0000000000000000{clock_rate:08x}"
    )
}

pub fn alac_caps(clock_rate: u32, channels: u8, bits: u8) -> String {
    format!(
        "audio/x-alac,rate=(int){clock_rate},channels=(int){channels},samplesize=(int){bits},\
         codec_data=(buffer){}",
        alac_cookie(clock_rate, channels, bits)
    )
}

pub fn aac_raw_caps(asc: &[u8], clock_rate: u32, channels: u8) -> String {
    let hex = livi_base::hex::encode(asc);
    format!(
        "audio/mpeg,mpegversion=(int)4,stream-format=(string)raw,framed=(boolean)true,\
         rate=(int){clock_rate},channels=(int){channels},codec_data=(buffer){hex}"
    )
}

pub fn rtp_caps(codec: Codec, payload_type: u8, clock_rate: u32, channels: u8) -> String {
    if codec == Codec::Pcm || codec == Codec::PcmLe {
        return lpcm_caps(codec, clock_rate, channels);
    }
    if codec == Codec::Opus {
        let mut caps = String::from("application/x-rtp,media=audio");
        caps.push_str(&format!(",clock-rate={clock_rate}"));
        caps.push_str(",encoding-name=OPUS");
        caps.push_str(&format!(",payload={payload_type}"));
        return caps;
    }

    let config = if codec == Codec::AacEld {
        eld_config(clock_rate, channels)
    } else {
        let asc = (2u16 << 11) | (aac_freq_index(clock_rate) << 7) | ((channels as u16) << 3);
        format!("{asc:04x}")
    };
    let mut caps = String::from("application/x-rtp,media=(string)audio");
    caps.push_str(&format!(",clock-rate=(int){clock_rate}"));
    caps.push_str(",encoding-name=(string)MPEG4-GENERIC,mode=(string)AAC-hbr");
    caps.push_str(&format!(",config=(string){config}"));
    caps.push_str(",sizelength=(string)13,indexlength=(string)3,indexdeltalength=(string)3");
    caps.push_str(&format!(",payload=(int){payload_type}"));
    caps
}

pub fn reframe_aac(rtp: &[u8], payload_type: u8) -> Vec<u8> {
    if rtp.len() < RTP_HEADER_LEN {
        return rtp.to_vec();
    }
    let au = &rtp[RTP_HEADER_LEN..];
    let au_header = ((au.len() << 3) & 0xffff) as u16;

    let mut out = Vec::with_capacity(rtp.len() + 4);
    out.extend_from_slice(&rtp[..RTP_HEADER_LEN]);
    out.extend_from_slice(&[0x00, 0x10, (au_header >> 8) as u8, au_header as u8]);
    out.extend_from_slice(au);
    out[1] = 0x80 | (payload_type & 0x7f);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn airplay_decoder_configs_follow_the_sample_rate() {
        assert_eq!(eld_config(44100, 2), "f8e85000");
        assert_eq!(eld_config(48000, 2), "f8e65000");
        assert_eq!(
            alac_cookie(44100, 2, 16),
            "00000024616c616300000000000001600010280a0e0200ff00000000000000000000ac44"
        );
        assert!(alac_cookie(48000, 2, 16).ends_with("0000bb80"));
        assert!(alac_cookie(44100, 2, 24).starts_with("00000024616c61630000000000000160001828"));
    }

    use super::*;
    use livi_crypto::chacha_seal;
    use std::sync::{Arc, Mutex};

    const KEY: [u8; 32] = [9u8; 32];

    #[derive(Default)]
    struct Reports {
        started: Vec<u32>,
        rtp: Vec<(Vec<u8>, u32)>,
    }

    #[derive(Default, Clone)]
    struct Seen(Arc<Mutex<Reports>>);

    impl Seen {
        fn started(&self) -> Vec<u32> {
            self.0.lock().unwrap().started.clone()
        }

        fn payloads(&self) -> Vec<(Vec<u8>, u32)> {
            self.0
                .lock()
                .unwrap()
                .rtp
                .iter()
                .map(|(p, s)| (p[RTP_HEADER_LEN..].to_vec(), *s))
                .collect()
        }
    }

    impl AudioSink for Seen {
        fn on_started(&mut self, first_sample: u32) {
            self.0.lock().unwrap().started.push(first_sample);
        }

        fn on_rtp(&mut self, rtp: &[u8], sample: u32) {
            self.0.lock().unwrap().rtp.push((rtp.to_vec(), sample));
        }
    }

    fn stream(seen: &Seen) -> AudioStream {
        AudioStream::new(KEY, Box::new(seen.clone()))
    }

    fn packet(sample: u32, nonce_counter: u64, plain: &[u8]) -> Vec<u8> {
        let mut header = vec![0x80, 0x60, 0x00, 0x00];
        header.extend_from_slice(&sample.to_be_bytes());
        header.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);

        let sealed = chacha_seal(&KEY, &nonce64(nonce_counter), plain, &header[4..]);

        let mut pkt = header;
        pkt.extend_from_slice(&sealed);
        pkt.extend_from_slice(&nonce_counter.to_le_bytes());
        pkt
    }

    #[test]
    fn a_packet_arrives_decrypted_and_carries_its_sample_position() {
        let seen = Seen::default();
        let mut s = stream(&seen);

        s.push(&packet(4242, 0, b"first"));

        assert_eq!(seen.payloads(), vec![(b"first".to_vec(), 4242)]);
    }

    #[test]
    fn the_start_is_announced_once_with_the_first_sample_position() {
        let seen = Seen::default();
        let mut s = stream(&seen);

        s.push(&packet(100, 0, b"one"));
        s.push(&packet(200, 1, b"two"));

        assert_eq!(seen.started(), vec![100]);
        assert_eq!(seen.payloads().len(), 2);
    }

    #[test]
    fn a_packet_that_fails_authentication_is_dropped() {
        let seen = Seen::default();
        let mut s = stream(&seen);
        let mut bad = packet(1, 0, b"tampered");
        bad[RTP_HEADER_LEN] ^= 0xff;

        s.push(&bad);

        assert!(seen.payloads().is_empty());
        assert!(seen.started().is_empty());
    }

    #[test]
    fn a_datagram_too_short_for_header_and_tail_is_ignored() {
        let seen = Seen::default();
        let mut s = stream(&seen);

        s.push(&[0u8; RTP_HEADER_LEN + TAIL_LEN - 1]);

        assert!(seen.payloads().is_empty());
    }

    #[test]
    fn an_empty_payload_still_reports() {
        let seen = Seen::default();
        let mut s = stream(&seen);

        s.push(&packet(7, 0, b""));

        assert_eq!(seen.payloads(), vec![(Vec::new(), 7)]);
    }

    #[test]
    fn reset_makes_the_next_packet_the_first_again() {
        let seen = Seen::default();
        let mut s = stream(&seen);
        s.push(&packet(10, 0, b"one"));

        s.reset();
        s.push(&packet(20, 1, b"two"));

        assert_eq!(seen.started(), vec![10, 20]);
    }
}

#[cfg(test)]
mod wire {
    use super::*;

    #[test]
    fn opus_caps_name_the_clock_rate_and_payload_type() {
        let caps = rtp_caps(Codec::Opus, 97, 48000, 1);

        assert_eq!(
            caps,
            "application/x-rtp,media=audio,clock-rate=48000,encoding-name=OPUS,payload=97"
        );
    }

    #[test]
    fn aac_caps_carry_the_rfc_3640_fields_as_strings() {
        let caps = rtp_caps(Codec::AacLc, 96, 44100, 2);

        assert!(caps.contains("clock-rate=(int)44100"));
        assert!(caps.contains("mode=(string)AAC-hbr"));
        assert!(caps.contains("sizelength=(string)13"));
        assert!(caps.contains("payload=(int)96"));
    }

    #[test]
    fn the_aac_config_holds_object_type_frequency_index_and_channels() {
        let caps = rtp_caps(Codec::AacLc, 96, 44100, 2);
        let asc = (2u16 << 11) | (4u16 << 7) | (2u16 << 3);

        assert!(caps.contains(&format!("config=(string){asc:04x}")));
    }

    #[test]
    fn an_unknown_rate_falls_back_to_the_48k_index() {
        assert_eq!(aac_freq_index(11025), aac_freq_index(48000));
    }

    #[test]
    fn carplay_samples_are_big_endian_and_handed_over_ones_are_not() {
        assert!(rtp_caps(Codec::Pcm, 0, 44100, 2).contains("format=S16BE"));
        assert!(rtp_caps(Codec::PcmLe, 0, 44100, 2).contains("format=S16LE"));
    }

    #[test]
    fn the_reframe_inserts_the_au_header_and_sets_marker_and_payload_type() {
        let mut rtp = vec![0x80, 0x60, 0, 0, 0, 0, 0, 1, 1, 2, 3, 4];
        rtp.extend_from_slice(&[0xaa; 5]);

        let out = reframe_aac(&rtp, 96);

        assert_eq!(out.len(), rtp.len() + 4);
        assert_eq!(out[1], 0x80 | 96);
        assert_eq!(&out[12..16], &[0x00, 0x10, 0x00, (5 << 3) as u8]);
        assert_eq!(&out[16..], &[0xaa; 5]);
    }

    #[test]
    fn a_packet_without_a_header_is_left_alone() {
        assert_eq!(reframe_aac(&[1, 2, 3], 96), vec![1, 2, 3]);
    }
}

pub mod receiver {
    use super::AudioStream;
    use socket2::{Domain, Protocol, Socket, Type};
    use std::net::UdpSocket;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::Duration;

    const DATAGRAM: usize = 4096;
    const POLL: Duration = Duration::from_millis(200);

    pub struct AudioReceiver {
        stop: Arc<AtomicBool>,
        threads: Vec<JoinHandle<()>>,
    }

    fn bind_any_port() -> std::io::Result<UdpSocket> {
        let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_only_v6(false)?;
        socket.bind(&"[::]:0".parse::<std::net::SocketAddr>().unwrap().into())?;
        let sock: UdpSocket = socket.into();
        sock.set_read_timeout(Some(POLL))?;
        Ok(sock)
    }

    impl AudioReceiver {
        pub fn new(stream: AudioStream) -> std::io::Result<(Self, u16, u16)> {
            let data = bind_any_port()?;
            let control = bind_any_port()?;
            let data_port = data.local_addr()?.port();
            let control_port = control.local_addr()?.port();

            let stop = Arc::new(AtomicBool::new(false));
            let mut threads = Vec::new();

            let data_stop = stop.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("cp-audio-rx".into())
                    .spawn(move || {
                        set_realtime();
                        let mut stream = stream;
                        let mut buf = [0u8; DATAGRAM];
                        while !data_stop.load(Ordering::Relaxed) {
                            if let Ok(n) = data.recv(&mut buf) {
                                stream.push(&buf[..n]);
                            }
                        }
                    })?,
            );

            let control_stop = stop.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("cp-audio-rtcp".into())
                    .spawn(move || {
                        let mut buf = [0u8; DATAGRAM];
                        while !control_stop.load(Ordering::Relaxed) {
                            let _ = control.recv(&mut buf);
                        }
                    })?,
            );

            Ok((Self { stop, threads }, data_port, control_port))
        }
    }

    impl Drop for AudioReceiver {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            for t in self.threads.drain(..) {
                let _ = t.join();
            }
        }
    }

    fn set_realtime() {
        // SAFETY: an all-zero sched_param is valid.
        let mut param: libc::sched_param = unsafe { std::mem::zeroed() };
        param.sched_priority = 10;
        // SAFETY: param is a valid sched_param for the calling thread.
        let rc =
            unsafe { libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param) };
        if rc != 0 {
            eprintln!("[cp_audio_rx] real-time priority denied (rc={rc}); normal priority");
        }
    }
}
