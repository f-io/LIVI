// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! AirPlay 2 buffered audio (stream type 103): sealed frames over TCP, played from the anchor the
//! sender sets, paused and flushed over RTSP.

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use livi_crypto::{ChaCha, TAG_LEN, nonce64};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::{AirplayCodec, AirplayFormat, AirplayMedia, CONNECT_WITHIN, TAG};
use crate::net;

const HEADER_LEN: usize = 12;
const NONCE_LEN: usize = 8;
const MAX_FRAME: usize = 64 * 1024;
const SEQ_BITS: u32 = 23;
const LEAD: Duration = Duration::from_millis(200);
const KEPT: Duration = Duration::from_secs(1);

const ALAC_48K_24: u32 = 0x1500_0000;
const AAC_LC_44K: u32 = 0x1600_0000;
const AAC_LC_48K: u32 = 0x1700_0000;
const AAC_LC_48K_5_1: u32 = 0x2700_0000;
const AAC_LC_48K_7_1: u32 = 0x2800_0000;

fn format_of(ssrc: u32) -> Option<AirplayFormat> {
    use AirplayCodec::*;
    Some(match ssrc {
        ALAC_48K_24 => AirplayFormat::new(Alac, 48_000, 24, 2),
        AAC_LC_44K => AirplayFormat::new(AacLc, 44_100, 16, 2),
        AAC_LC_48K => AirplayFormat::new(AacLc, 48_000, 16, 2),
        AAC_LC_48K_5_1 => AirplayFormat::new(AacLc, 48_000, 16, 6),
        AAC_LC_48K_7_1 => AirplayFormat::new(AacLc, 48_000, 16, 8),
        _ => return None,
    })
}

fn seq_after(a: u32, b: u32) -> i32 {
    (a.wrapping_sub(b) << (32 - SEQ_BITS)) as i32 >> (32 - SEQ_BITS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flush {
    from: Option<u32>,
    until: u32,
}

impl Flush {
    fn drops(self, seq: u32) -> bool {
        self.from.is_none_or(|f| seq_after(seq, f) >= 0) && seq_after(seq, self.until) <= 0
    }
}

#[derive(Debug, Clone, Default)]
struct Control {
    anchor: Option<(u32, Instant)>,
    flush: Option<Flush>,
    flushes: u64,
    halts: u64,
}

pub struct Buffered {
    control: watch::Sender<Control>,
    task: JoinHandle<()>,
}

impl Buffered {
    pub fn listen(
        key: [u8; 32],
        fallback: AirplayFormat,
        media: Arc<dyn AirplayMedia>,
    ) -> io::Result<(Self, u16)> {
        let listener = net::tcp_listener()?;
        let port = net::local_port(listener.local_addr());
        let (control, seen) = watch::channel(Control::default());
        let task = tokio::spawn(async move {
            let Ok(Ok((stream, _))) = tokio::time::timeout(CONNECT_WITHIN, listener.accept()).await
            else {
                return;
            };
            let mut player = Player {
                key: ChaCha::new(&key),
                fallback,
                media,
                seen,
                format: None,
                halts: 0,
                flushes: 0,
                flush: None,
                kept: VecDeque::new(),
            };
            if let Err(e) = player.run(stream).await {
                println!("{TAG} buffered audio: {e}");
            }
        });
        Ok((Self { control, task }, port))
    }

    pub fn rate(&self, rtp: Option<u32>) {
        self.control.send_modify(|c| {
            c.anchor = rtp.map(|r| (r, Instant::now()));
            c.halts += 1;
        });
    }

    pub fn flush(&self, from: Option<u32>, until: u32) {
        self.control.send_modify(|c| {
            c.flush = Some(Flush { from, until });
            c.flushes += 1;
            c.halts += 1;
        });
    }
}

impl Drop for Buffered {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Frame {
    seq: u32,
    ts: u32,
    format: AirplayFormat,
    audio: Vec<u8>,
}

fn open(key: &ChaCha, fallback: AirplayFormat, body: &mut [u8]) -> Option<Frame> {
    let end = body.len().checked_sub(NONCE_LEN)?;
    if end < HEADER_LEN + TAG_LEN {
        return None;
    }
    let be = |at: usize| u32::from_be_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]]);
    let (seq, ts, ssrc) = (be(0) & ((1 << SEQ_BITS) - 1), be(4), be(8));
    if ssrc == 0 {
        return None;
    }
    let counter = u64::from_le_bytes(body[end..].try_into().ok()?);
    let (head, sealed) = body[..end].split_at_mut(HEADER_LEN);
    let Some(audio) = key.open_in_place(&nonce64(counter), sealed, &head[4..]) else {
        println!("{TAG} buffered frame {seq} failed authentication");
        return None;
    };
    Some(Frame {
        seq,
        ts,
        format: format_of(ssrc).unwrap_or(fallback),
        audio: audio.to_vec(),
    })
}

struct Player {
    key: ChaCha,
    fallback: AirplayFormat,
    media: Arc<dyn AirplayMedia>,
    seen: watch::Receiver<Control>,
    format: Option<AirplayFormat>,
    halts: u64,
    flushes: u64,
    flush: Option<Flush>,
    kept: VecDeque<Frame>,
}

impl Player {
    async fn run(&mut self, mut stream: TcpStream) -> io::Result<()> {
        let mut buf = vec![0u8; MAX_FRAME];
        loop {
            let len = usize::from(stream.read_u16().await?);
            let Some(body) = len.checked_sub(2).and_then(|n| buf.get_mut(..n)) else {
                return Err(io::Error::other(format!("frame of {len} bytes")));
            };
            stream.read_exact(body).await?;
            if let Some(frame) = open(&self.key, self.fallback, body) {
                self.play(frame).await;
            }
        }
    }

    async fn play(&mut self, frame: Frame) {
        loop {
            let control = self.seen.borrow_and_update().clone();
            if control.flushes != self.flushes {
                self.flushes = control.flushes;
                self.flush = control.flush;
                self.kept.clear();
            }
            if let Some(flush) = self.flush {
                if flush.drops(frame.seq) {
                    return;
                }
                if seq_after(frame.seq, flush.until) > 0 {
                    self.flush = None;
                }
            }
            let Some((anchor, at)) = control.anchor else {
                let _ = self.seen.changed().await;
                continue;
            };
            let ahead = frame.ts.wrapping_sub(anchor) as i32;
            if ahead < 0 {
                return;
            }
            if control.halts != self.halts || self.format != Some(frame.format) {
                self.halts = control.halts;
                self.reopen(frame.format, anchor).await;
                continue;
            }
            let rate = u64::from(frame.format.rate.max(1));
            let due = at + Duration::from_micros(ahead as u64 * 1_000_000 / rate);
            if due > Instant::now() + LEAD {
                tokio::select! {
                    () = tokio::time::sleep_until(due - LEAD) => {}
                    _ = self.seen.changed() => {}
                }
                continue;
            }
            self.media.push_buffered(&frame.audio);
            self.keep(frame);
            return;
        }
    }

    async fn reopen(&mut self, format: AirplayFormat, anchor: u32) {
        if self.format != Some(format) {
            println!(
                "{TAG} buffered audio {:?} at {} Hz/{} bit, {} channels",
                format.codec, format.rate, format.bits, format.channels
            );
        }
        self.format = Some(format);
        self.media.open_buffered(format).await;
        for kept in &self.kept {
            if kept.format == format && kept.ts.wrapping_sub(anchor) as i32 >= 0 {
                self.media.push_buffered(&kept.audio);
            }
        }
    }

    fn keep(&mut self, frame: Frame) {
        let span = u64::from(frame.format.rate) * KEPT.as_millis() as u64 / 1000;
        let newest = frame.ts;
        self.kept.push_back(frame);
        while self
            .kept
            .front()
            .is_some_and(|f| u64::from(newest.wrapping_sub(f.ts)) > span)
        {
            self.kept.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_numbers_wrap_at_23_bits() {
        assert_eq!(seq_after(0, (1 << SEQ_BITS) - 1), 1);
        assert_eq!(seq_after(5, 3), 2);
        assert_eq!(seq_after(3, 5), -2);
    }

    #[test]
    fn an_immediate_flush_drops_up_to_its_end() {
        let flush = Flush {
            from: None,
            until: 100,
        };
        assert!(flush.drops(40));
        assert!(flush.drops(100));
        assert!(!flush.drops(101));
    }

    #[test]
    fn a_deferred_flush_drops_only_its_range() {
        let flush = Flush {
            from: Some(50),
            until: 60,
        };
        assert!(!flush.drops(49));
        assert!(flush.drops(55));
        assert!(!flush.drops(61));
    }

    #[test]
    fn a_frame_opens_with_the_stream_key() {
        let key = [7u8; 32];
        let mut frame = vec![0x80, 0, 0, 9, 0, 0, 1, 0, 0x17, 0, 0, 0];
        let sealed = ChaCha::new(&key).seal(&nonce64(3), b"aac", &frame[4..12]);
        frame.extend_from_slice(&sealed);
        frame.extend_from_slice(&3u64.to_le_bytes());
        let fallback = AirplayFormat::new(AirplayCodec::Alac, 44_100, 16, 2);
        let opened = open(&ChaCha::new(&key), fallback, &mut frame).unwrap();
        assert_eq!((opened.seq, opened.ts), (9, 256));
        assert_eq!(opened.format, format_of(AAC_LC_48K).unwrap());
        assert_eq!(opened.audio, b"aac");
    }
}
