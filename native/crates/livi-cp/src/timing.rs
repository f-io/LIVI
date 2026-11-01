// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::{Instant, SystemTime, UNIX_EPOCH};

const PT_REQUEST: u8 = 210;
const PT_RESPONSE: u8 = 211;
const NTP_EPOCH_OFFSET: u64 = 2_208_988_800;
const TWO32: f64 = 4_294_967_296.0;
const STEP_THRESHOLD_SEC: f64 = 0.128;
const SLEW_GAIN: f64 = 1.0 / 8.0;
const DELAY_WINDOW: usize = 8;
const PICK_COUNT: u32 = 2;
pub const PACKET_LEN: usize = 32;

pub fn ntp64_now() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let sec = since.as_secs() + NTP_EPOCH_OFFSET;
    let frac = (u64::from(since.subsec_nanos()) << 32) / 1_000_000_000;
    (sec << 32) | frac
}

fn ns_to_ntp(ns: i128) -> u64 {
    let ns = ns.max(0) as u128;
    let sec = ns / 1_000_000_000;
    let frac = ((ns % 1_000_000_000) << 32) / 1_000_000_000;
    ((sec << 32) | frac) as u64
}

fn ntp_to_ns(ntp: u64) -> i128 {
    i128::from(ntp >> 32) * 1_000_000_000 + ((i128::from(ntp & 0xffff_ffff) * 1_000_000_000) >> 32)
}

fn write_ntp(pkt: &mut [u8; PACKET_LEN], at: usize, ntp: u64) {
    pkt[at..at + 8].copy_from_slice(&ntp.to_be_bytes());
}

fn read_ntp(pkt: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(pkt[at..at + 8].try_into().expect("8 bytes"))
}

fn header(kind: u8) -> [u8; PACKET_LEN] {
    let mut pkt = [0u8; PACKET_LEN];
    pkt[0] = 0x80;
    pkt[1] = kind;
    pkt[2..4].copy_from_slice(&7u16.to_be_bytes());
    pkt
}

pub struct TimingClock {
    base: Instant,
    offset_ns: i128,
    delays: [f64; DELAY_WINDOW],
    delay_index: usize,
    pick_left: u32,
    pick_rtt: f64,
    pick_offset: f64,
    pending_t1: Option<u64>,
    synced: bool,
}

impl Default for TimingClock {
    fn default() -> Self {
        Self::new()
    }
}

impl TimingClock {
    pub fn new() -> Self {
        let base = Instant::now();
        Self {
            base,
            offset_ns: ntp_to_ns(ntp64_now()),
            delays: [f64::INFINITY; DELAY_WINDOW],
            delay_index: 0,
            pick_left: PICK_COUNT,
            pick_rtt: f64::INFINITY,
            pick_offset: 0.0,
            pending_t1: None,
            synced: false,
        }
    }

    fn mono_ns(&self) -> i128 {
        self.base.elapsed().as_nanos() as i128
    }

    pub fn synced_ntp(&self) -> u64 {
        ns_to_ntp(self.mono_ns() + self.offset_ns)
    }

    pub fn synced(&self) -> bool {
        self.synced
    }

    pub fn request(&mut self) -> [u8; PACKET_LEN] {
        let mut pkt = header(PT_REQUEST);
        let t1 = self.synced_ntp();
        self.pending_t1 = Some(t1);
        write_ntp(&mut pkt, 24, t1);
        pkt
    }

    pub fn on_packet(&mut self, msg: &[u8]) -> Option<[u8; PACKET_LEN]> {
        if msg.len() < PACKET_LEN {
            return None;
        }
        match msg[1] {
            PT_REQUEST => {
                let mut resp = header(PT_RESPONSE);
                resp[8..16].copy_from_slice(&msg[24..32]);
                write_ntp(&mut resp, 16, self.synced_ntp());
                write_ntp(&mut resp, 24, self.synced_ntp());
                Some(resp)
            }
            PT_RESPONSE => {
                let t4 = self.synced_ntp();
                self.on_response(read_ntp(msg, 8), read_ntp(msg, 16), read_ntp(msg, 24), t4);
                None
            }
            _ => None,
        }
    }

    fn on_response(&mut self, t1: u64, t2: u64, t3: u64, t4: u64) {
        if self.pending_t1 != Some(t1) {
            return;
        }
        self.pending_t1 = None;
        let d = |a: u64, b: u64| (i128::from(a) - i128::from(b)) as f64;
        let offset = 0.5 * (d(t2, t1) + d(t3, t4)) / TWO32;
        let rtt = (d(t4, t1) - d(t3, t2)) / TWO32;
        if rtt < 0.0 {
            return;
        }

        if rtt < self.pick_rtt {
            self.pick_rtt = rtt;
            self.pick_offset = offset;
        }
        self.pick_left -= 1;
        if self.pick_left > 0 {
            return;
        }
        let (rtt, offset) = (self.pick_rtt, self.pick_offset);
        self.pick_left = PICK_COUNT;
        self.pick_rtt = f64::INFINITY;

        let use_sample = self.delays.iter().all(|&d| rtt <= d);
        self.delays[self.delay_index] = rtt;
        self.delay_index = (self.delay_index + 1) % DELAY_WINDOW;
        if use_sample {
            self.apply(offset, rtt);
        }
    }

    fn apply(&mut self, offset: f64, rtt: f64) {
        let stepping = !self.synced || offset.abs() > STEP_THRESHOLD_SEC;
        let applied = if stepping { offset } else { offset * SLEW_GAIN };
        self.offset_ns += (applied * 1e9).round() as i128;
        if stepping {
            self.delays = [f64::INFINITY; DELAY_WINDOW];
            self.delay_index = 0;
            self.pending_t1 = None;
            println!(
                "[cp] clock {}: offset {offset:.3} s, rtt {:.1} ms",
                if self.synced { "stepped" } else { "synced" },
                rtt * 1000.0
            );
            self.synced = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(req: &[u8; PACKET_LEN], skew_ns: i128, rtt_ns: i128) -> [u8; PACKET_LEN] {
        let t1 = read_ntp(req, 24);
        let t2 = ns_to_ntp(ntp_to_ns(t1) + skew_ns + rtt_ns / 2);
        let mut resp = header(PT_RESPONSE);
        write_ntp(&mut resp, 8, t1);
        write_ntp(&mut resp, 16, t2);
        write_ntp(&mut resp, 24, t2);
        resp
    }

    #[test]
    fn the_first_pick_steps_onto_the_phone_clock() {
        let mut clock = TimingClock::new();
        let skew = 5_000_000_000i128;
        for _ in 0..PICK_COUNT {
            let req = clock.request();
            clock.on_packet(&answer(&req, skew, 1_000_000));
        }
        assert!(clock.synced());
        let ahead = ntp_to_ns(clock.synced_ntp()) - ntp_to_ns(ntp64_now());
        assert!(
            (ahead - skew).abs() < 50_000_000,
            "clock is {ahead} ns ahead"
        );
    }

    #[test]
    fn a_stale_or_negative_answer_is_ignored() {
        let mut clock = TimingClock::new();
        let old = clock.request();
        let _new = clock.request();
        clock.on_packet(&answer(&old, 1_000_000_000, 0));
        clock.on_packet(&answer(&old, 1_000_000_000, 0));
        assert!(!clock.synced());

        let req = clock.request();
        let mut neg = answer(&req, 0, 0);
        let late = read_ntp(&neg, 16) + (1 << 40);
        write_ntp(&mut neg, 24, late);
        clock.on_packet(&neg);
        assert!(!clock.synced());
        assert_eq!(clock.on_packet(&[0u8; 10]), None);
    }

    #[test]
    fn a_request_from_the_phone_is_answered() {
        let mut clock = TimingClock::new();
        let mut req = header(PT_REQUEST);
        write_ntp(&mut req, 24, 0x0102_0304_0506_0708);
        let resp = clock.on_packet(&req).unwrap();
        assert_eq!(resp[1], PT_RESPONSE);
        assert_eq!(&resp[2..4], &[0, 7]);
        assert_eq!(read_ntp(&resp, 8), 0x0102_0304_0506_0708);
        assert!(read_ntp(&resp, 24) >= read_ntp(&resp, 16));
    }

    #[test]
    fn ntp_conversion_round_trips() {
        let ns = 3_900_000_000_123_456_789i128;
        assert!((ntp_to_ns(ns_to_ntp(ns)) - ns).abs() <= 1);
        assert_eq!(ns_to_ntp(-5), 0);
    }
}
