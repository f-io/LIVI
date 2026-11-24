// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! The sender's PTP clock (IEEE 1588), followed passively from its Follow_Up messages. The ports
//! are fixed by the standard and other receivers on the host may need them, so they are held only
//! while a stream wants the clock, and a stream whose bind fails stays on local time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::TAG;
use crate::net;

const EVENT_PORT: u16 = 319;
const GENERAL_PORT: u16 = 320;
const FOLLOW_UP: u8 = 0x8;
const ANNOUNCE: u8 = 0xB;
const HEADER_LEN: usize = 34;
const TIMESTAMP_END: usize = HEADER_LEN + 10;
const GRANDMASTER_AT: usize = 53;
const STALE: Duration = Duration::from_secs(5);
const FORGET: Duration = Duration::from_secs(60);
const SETTLE: Duration = Duration::from_secs(1);
const RISE_SHARE: i64 = 16;
const FALL_SHARE: i64 = 256;
const FALL_LIMIT_NS: i64 = 2_500_000;

static SHARED: Mutex<Weak<Listener>> = Mutex::new(Weak::new());

struct Clock {
    offset_ns: i64,
    since: Duration,
    heard: Duration,
}

impl Clock {
    fn follow(&mut self, sample: i64, at: Duration) {
        let diff = sample - self.offset_ns;
        let settling = at.saturating_sub(self.since) < SETTLE;
        self.offset_ns += match (diff > 0, settling) {
            (true, true) => diff,
            (true, false) => diff / RISE_SHARE,
            (false, true) => 0,
            (false, false) => diff.max(-FALL_LIMIT_NS) / FALL_SHARE,
        };
        self.heard = at;
    }
}

#[derive(Default)]
struct Clocks {
    clocks: HashMap<u64, Clock>,
    grandmasters: HashMap<u64, u64>,
}

fn be(msg: &[u8], at: usize, len: usize) -> u64 {
    msg[at..at + len]
        .iter()
        .fold(0, |n, &b| (n << 8) | u64::from(b))
}

impl Clocks {
    fn note(&mut self, msg: &[u8], at: Duration) -> Option<u64> {
        if msg.len() < HEADER_LEN {
            return None;
        }
        let id = be(msg, 20, 8);
        match msg[0] & 0x0f {
            ANNOUNCE if msg.len() >= GRANDMASTER_AT + 8 => {
                self.grandmasters.insert(id, be(msg, GRANDMASTER_AT, 8));
                None
            }
            FOLLOW_UP if msg.len() >= TIMESTAMP_END => {
                let correction = be(msg, 8, 8) as i64 >> 16;
                let remote = be(msg, 34, 6) * 1_000_000_000 + be(msg, 40, 4);
                let sample = remote as i64 + correction - at.as_nanos() as i64;
                self.clocks
                    .retain(|_, c| at.saturating_sub(c.heard) < FORGET);
                match self.clocks.get_mut(&id) {
                    Some(c) if at.saturating_sub(c.heard) < STALE => {
                        c.follow(sample, at);
                        None
                    }
                    _ => {
                        let clock = Clock {
                            offset_ns: sample,
                            since: at,
                            heard: at,
                        };
                        self.clocks.insert(id, clock);
                        Some(id)
                    }
                }
            }
            _ => None,
        }
    }

    fn local(&self, timeline: u64, ns: u64, now: Duration) -> Option<Duration> {
        let (_, clock) = self
            .clocks
            .iter()
            .find(|(id, _)| **id == timeline || self.grandmasters.get(id) == Some(&timeline))?;
        if now.saturating_sub(clock.heard) >= STALE {
            return None;
        }
        let local = i64::try_from(ns).ok()? - clock.offset_ns;
        Some(Duration::from_nanos(u64::try_from(local).ok()?))
    }
}

struct Listener {
    clocks: Arc<Mutex<Clocks>>,
    base: Instant,
    tasks: [JoinHandle<()>; 2],
}

impl Drop for Listener {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        println!("{TAG} ptp ports released");
    }
}

pub struct Ptp(Arc<Listener>);

impl Ptp {
    pub fn join() -> Option<Self> {
        let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(listener) = shared.upgrade() {
            return Some(Self(listener));
        }
        let bound = net::udp_socket_at(EVENT_PORT)
            .and_then(|event| Ok((event, net::udp_socket_at(GENERAL_PORT)?)));
        let (event, general) = match bound {
            Ok(sockets) => sockets,
            Err(e) => {
                println!(
                    "{TAG} ptp ports {EVENT_PORT}/{GENERAL_PORT}: {e}, the stream follows local time"
                );
                return None;
            }
        };
        println!("{TAG} ptp listening");
        let clocks = Arc::new(Mutex::new(Clocks::default()));
        let base = Instant::now();
        let listener = Arc::new(Listener {
            clocks: clocks.clone(),
            base,
            tasks: [
                tokio::spawn(drain(event)),
                tokio::spawn(follow(general, clocks, base)),
            ],
        });
        *shared = Arc::downgrade(&listener);
        Some(Self(listener))
    }

    pub fn local(&self, timeline: u64, ns: u64) -> Option<Instant> {
        let now = self.0.base.elapsed();
        let clocks = self.0.clocks.lock().unwrap_or_else(PoisonError::into_inner);
        clocks.local(timeline, ns, now).map(|at| self.0.base + at)
    }
}

async fn drain(sock: UdpSocket) {
    let mut buf = [0u8; 256];
    while sock.recv_from(&mut buf).await.is_ok() {}
}

async fn follow(sock: UdpSocket, clocks: Arc<Mutex<Clocks>>, base: Instant) {
    let mut buf = [0u8; 256];
    while let Ok((len, from)) = sock.recv_from(&mut buf).await {
        if from.port() != GENERAL_PORT {
            continue;
        }
        let at = base.elapsed();
        let mut clocks = clocks.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(id) = clocks.note(&buf[..len], at) {
            println!("{TAG} ptp clock {id:016x} from {}", net::host_of(&from));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: u64 = 0x0011_22ff_fe33_4455;
    const GM: u64 = 0x0066_77ff_fe88_99aa;
    const S: Duration = Duration::from_secs(1);

    fn header(kind: u8, len: usize) -> Vec<u8> {
        let mut msg = vec![0u8; len];
        msg[0] = kind;
        msg[20..28].copy_from_slice(&ID.to_be_bytes());
        msg
    }

    fn follow_up(remote_ns: u64) -> Vec<u8> {
        let mut msg = header(FOLLOW_UP, TIMESTAMP_END);
        let secs = remote_ns / 1_000_000_000;
        msg[34..40].copy_from_slice(&secs.to_be_bytes()[2..]);
        msg[40..44].copy_from_slice(&((remote_ns % 1_000_000_000) as u32).to_be_bytes());
        msg
    }

    #[test]
    fn a_follow_up_maps_the_senders_time_onto_ours() {
        let mut clocks = Clocks::default();
        assert_eq!(
            clocks.note(&follow_up(500 * S.as_nanos() as u64), S),
            Some(ID)
        );
        let at = clocks.local(ID, 502 * S.as_nanos() as u64, S).unwrap();
        assert_eq!(at, 3 * S);
    }

    #[test]
    fn a_late_reading_barely_moves_the_clock() {
        let mut clocks = Clocks::default();
        clocks.note(&follow_up(100 * S.as_nanos() as u64), S);
        let late = 1_000_000;
        clocks.note(&follow_up(102 * S.as_nanos() as u64 - late), 3 * S);
        let offset = clocks.clocks[&ID].offset_ns;
        assert_eq!(offset, 99 * S.as_nanos() as i64 - late as i64 / FALL_SHARE);
    }

    #[test]
    fn the_anchor_may_name_the_grandmaster() {
        let mut clocks = Clocks::default();
        let mut announce = header(ANNOUNCE, 64);
        announce[GRANDMASTER_AT..GRANDMASTER_AT + 8].copy_from_slice(&GM.to_be_bytes());
        clocks.note(&announce, S);
        clocks.note(&follow_up(10 * S.as_nanos() as u64), S);
        assert!(clocks.local(GM, 11 * S.as_nanos() as u64, S).is_some());
    }

    #[test]
    fn a_silent_clock_places_no_anchor() {
        let mut clocks = Clocks::default();
        clocks.note(&follow_up(10 * S.as_nanos() as u64), S);
        assert!(
            clocks
                .local(ID, 11 * S.as_nanos() as u64, S + STALE)
                .is_none()
        );
    }
}
