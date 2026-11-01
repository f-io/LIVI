// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::Duration;

use tokio::time::Instant;

use super::info::{FixMode, FixQuality, GnssInfo, GpsFix, Rf, Satellite, Version};
use super::nmea::Decoder;
use super::ubx;

pub const DEFAULT_DEVICE: &str = "/dev/ttyAMA0";
pub const DEFAULT_BAUD_RATE: u32 = 38400;

const RETRY: Duration = Duration::from_secs(5);
const STALE: Duration = Duration::from_secs(10);
const INFO_EVERY: Duration = Duration::from_secs(1);
const VERSION_POLL_EVERY: Duration = Duration::from_secs(2);
const VERSION_POLL_TRIES: u32 = 5;
const RF_POLL_EVERY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
    Version,
    Rf,
}

#[derive(Debug, PartialEq)]
pub enum Output {
    Open,
    Close,
    Poll(Poll),
    Info(Box<GnssInfo>),
    Fix(GpsFix),
}

pub struct Receiver {
    device: String,
    baud_rate: u32,
    nmea: Decoder,
    ubx: ubx::Parser,
    running: bool,
    port_open: bool,
    receiving: bool,
    retry_at: Option<Instant>,
    stale_at: Option<Instant>,
    info_at: Option<Instant>,
    version_at: Option<Instant>,
    rf_at: Option<Instant>,
    version_polls: u32,
    version: Option<Version>,
    rf: Option<Rf>,
    fix_quality: FixQuality,
    fix_mode: FixMode,
    satellites: Vec<Satellite>,
    satellites_used: f64,
    pdop: Option<f64>,
    hdop: Option<f64>,
    vdop: Option<f64>,
    receiver_time: Option<f64>,
    accuracy_m: Option<f64>,
    last_error: Option<String>,
    updated_at: Option<u64>,
    out: Vec<Output>,
}

impl Receiver {
    pub fn new(device: &str, baud_rate: u32) -> Self {
        Self {
            device: device.to_string(),
            baud_rate,
            nmea: Decoder::default(),
            ubx: ubx::Parser::default(),
            running: false,
            port_open: false,
            receiving: false,
            retry_at: None,
            stale_at: None,
            info_at: None,
            version_at: None,
            rf_at: None,
            version_polls: 0,
            version: None,
            rf: None,
            fix_quality: FixQuality::None,
            fix_mode: FixMode::None,
            satellites: Vec::new(),
            satellites_used: 0.0,
            pdop: None,
            hdop: None,
            vdop: None,
            receiver_time: None,
            accuracy_m: None,
            last_error: None,
            updated_at: None,
            out: Vec::new(),
        }
    }

    pub fn take(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }

    pub fn start(&mut self) {
        if self.running {
            return;
        }
        self.running = true;
        self.open();
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.retry_at = None;
        self.stale_at = None;
        self.info_at = None;
        self.version_at = None;
        self.rf_at = None;
        self.out.push(Output::Close);
        if self.port_open || self.receiving {
            self.port_open = false;
            self.receiving = false;
            self.emit_info_now();
        }
    }

    pub fn info(&self) -> GnssInfo {
        GnssInfo {
            connected: self.receiving,
            device: Some(self.device.clone()),
            baud_rate: Some(self.baud_rate),
            fix_quality: self.fix_quality,
            fix_mode: self.fix_mode,
            satellites_used: self.satellites_used,
            satellites_visible: self.satellites.len(),
            satellites: self.satellites.clone(),
            constellations: self.nmea.constellations_seen(),
            version: self.version.clone(),
            rf: self.rf,
            error: self.last_error.clone(),
            pdop: self.pdop,
            hdop: self.hdop,
            vdop: self.vdop,
            receiver_time: self.receiver_time,
            updated_at: self.updated_at,
            timezone: None,
        }
    }

    fn open(&mut self) {
        if self.running && !self.port_open {
            self.out.push(Output::Open);
        }
    }

    pub fn opened(&mut self, now: Instant) {
        if !self.running || self.port_open {
            return;
        }
        self.last_error = None;
        self.port_open = true;
        self.stale_at = Some(now + STALE);
        self.emit_info_now();
        println!("[gnss] {} open at {} baud", self.device, self.baud_rate);
    }

    pub fn failed(&mut self, reason: String, now: Instant) {
        self.out.push(Output::Close);
        if self.last_error.as_ref() != Some(&reason) {
            eprintln!("[gnss] {reason}");
        }
        self.last_error = Some(reason);
        self.port_open = false;
        self.receiving = false;
        self.emit_info_now();
        if self.running && self.retry_at.is_none() {
            self.retry_at = Some(now + RETRY);
        }
    }

    pub fn data(&mut self, chunk: &[u8], now: Instant, unix_ms: u64) {
        self.stale_at = Some(now + STALE);
        self.updated_at = Some(unix_ms);

        if !self.receiving {
            self.receiving = true;
            self.version_polls = 0;
            self.ask_version(now);
            self.ask_rf(now);
            self.emit_info_now();
        }

        for frame in self.ubx.push(chunk) {
            if frame.class != ubx::CLASS_MON {
                continue;
            }
            if frame.id == ubx::ID_MON_RF {
                if let Some(rf) = ubx::parse_rf(&frame.payload) {
                    self.rf = Some(rf);
                    self.emit_info_now();
                }
                continue;
            }
            if frame.id != ubx::ID_MON_VER {
                continue;
            }
            if let Some(version) = ubx::parse_version(&frame.payload) {
                let name = or(
                    version.model.as_deref(),
                    or(Some(version.software.as_str()), "receiver"),
                );
                println!(
                    "[gnss] {name}, firmware {}, protocol {}",
                    or(version.firmware.as_deref(), "unknown"),
                    or(version.protocol.as_deref(), "unknown"),
                );
                self.version = Some(version);
                self.emit_info_now();
            }
        }

        let mut fix: Option<GpsFix> = None;
        for update in self.nmea.push(chunk) {
            if let Some(q) = update.fix_quality {
                self.fix_quality = q;
            }
            if let Some(m) = update.fix_mode {
                self.fix_mode = m;
            }
            if let Some(n) = update.satellites_used {
                self.satellites_used = n;
            }
            if let Some(s) = update.satellites {
                self.satellites = s;
            }
            self.pdop = update.pdop.or(self.pdop);
            self.hdop = update.hdop.or(self.hdop);
            self.vdop = update.vdop.or(self.vdop);
            self.receiver_time = update.receiver_time.or(self.receiver_time);
            if let Some(gps) = update.gps {
                self.accuracy_m = gps.accuracy_m.or(self.accuracy_m);
                fix.get_or_insert_default().merge(&gps);
            }
        }
        if let Some(mut fix) = fix.filter(|f| f.lat.is_some() && f.lng.is_some()) {
            fix.accuracy_m = self.accuracy_m.or(fix.accuracy_m);
            self.out.push(Output::Fix(fix));
        }

        self.emit_info_paced(now);
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        [
            self.stale_at,
            self.rf_at,
            self.version_at,
            self.info_at,
            self.retry_at,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn tick(&mut self, now: Instant) {
        let due = |at: &mut Option<Instant>| at.take_if(|at| *at <= now).is_some();
        if due(&mut self.stale_at) {
            self.went_silent();
        }
        if due(&mut self.rf_at) && self.receiving {
            self.ask_rf(now);
        }
        if due(&mut self.version_at) {
            self.ask_version(now);
        }
        if due(&mut self.info_at) {
            self.out.push(Output::Info(Box::new(self.info())));
        }
        if due(&mut self.retry_at) {
            self.open();
        }
    }

    fn went_silent(&mut self) {
        let was_receiving = self.receiving;
        self.receiving = false;
        self.version = None;
        self.rf = None;
        self.satellites.clear();
        if self.fix_quality == FixQuality::None && self.fix_mode == FixMode::None {
            if was_receiving {
                self.emit_info_now();
            }
            return;
        }
        eprintln!(
            "[gnss] no sentences for {}s, dropping the fix state",
            STALE.as_secs()
        );
        self.fix_quality = FixQuality::None;
        self.fix_mode = FixMode::None;
        self.satellites_used = 0.0;
        self.emit_info_now();
    }

    fn ask_version(&mut self, now: Instant) {
        if !self.running || self.version.is_some() || self.version_polls >= VERSION_POLL_TRIES {
            return;
        }
        self.version_polls += 1;
        self.out.push(Output::Poll(Poll::Version));
        self.version_at = Some(now + VERSION_POLL_EVERY);
    }

    fn ask_rf(&mut self, now: Instant) {
        if !self.running {
            return;
        }
        self.out.push(Output::Poll(Poll::Rf));
        self.rf_at = Some(now + RF_POLL_EVERY);
    }

    fn emit_info_now(&mut self) {
        self.info_at = None;
        self.out.push(Output::Info(Box::new(self.info())));
    }

    fn emit_info_paced(&mut self, now: Instant) {
        self.info_at.get_or_insert(now + INFO_EVERY);
    }
}

fn or<'a>(text: Option<&'a str>, fallback: &'a str) -> &'a str {
    text.filter(|t| !t.is_empty()).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gnss::info::{AntennaPower, AntennaStatus, Jamming};
    use crate::gnss::ubx::tests::{field, rf_payload};

    const GGA: &[u8] = b"$GPGGA,123519.00,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*69\r\n";
    const RMC: &[u8] = b"$GPRMC,123519.00,A,4807.038,N,01131.000,E,022.4,084.4,230326,,,A*5B\r\n";
    const GSA: &[u8] = b"$GPGSA,A,3,04,05,,09,12,,,24,,,,,2.5,1.3,2.1*39\r\n";
    const GST: &[u8] = b"$GPGST,123519.00,1.5,,,,3.0,4.0,2.0*75\r\n";

    struct Bench {
        r: Receiver,
        now: Instant,
        infos: Vec<GnssInfo>,
        fixes: Vec<GpsFix>,
        polls: Vec<Poll>,
        opens: usize,
        closes: usize,
    }

    impl Bench {
        fn new() -> Self {
            Self {
                r: Receiver::new(DEFAULT_DEVICE, DEFAULT_BAUD_RATE),
                now: Instant::now(),
                infos: Vec::new(),
                fixes: Vec::new(),
                polls: Vec::new(),
                opens: 0,
                closes: 0,
            }
        }

        fn open() -> Self {
            let mut b = Self::new();
            b.r.start();
            b.collect();
            assert_eq!(b.opens, 1);
            b.r.opened(b.now);
            b.collect();
            b
        }

        fn collect(&mut self) {
            for out in self.r.take() {
                match out {
                    Output::Open => self.opens += 1,
                    Output::Close => self.closes += 1,
                    Output::Poll(p) => self.polls.push(p),
                    Output::Info(i) => self.infos.push(*i),
                    Output::Fix(f) => self.fixes.push(f),
                }
            }
        }

        fn feed(&mut self, bytes: &[u8]) {
            self.r.data(bytes, self.now, 1_800_000_000_000);
            self.collect();
        }

        fn advance(&mut self, by: Duration) {
            let until = self.now + by;
            while let Some(at) = self.r.next_deadline().filter(|at| *at <= until) {
                self.now = at;
                self.r.tick(at);
                self.collect();
            }
            self.now = until;
        }
    }

    fn frame(class: u8, id: u8, payload: &[u8]) -> Vec<u8> {
        ubx::build(class, id, payload)
    }

    fn version_frame(firmware: &str) -> Vec<u8> {
        let payload = [
            field("ROM CORE 4.04", 30),
            field("00190000", 10),
            field(&format!("FWVER={firmware}"), 30),
        ];
        frame(0x0a, 0x04, &payload.concat())
    }

    #[test]
    fn connected_means_bytes_arrive() {
        let mut b = Bench::open();
        assert!(!b.r.info().connected);
        assert!(b.polls.is_empty());
        b.feed(GGA);
        assert!(b.r.info().connected);
        b.advance(STALE);
        assert!(!b.r.info().connected);
    }

    #[test]
    fn the_identity_and_the_rf_front_end_are_asked_for_on_the_first_bytes() {
        let mut b = Bench::open();
        b.advance(Duration::from_secs(120));
        assert!(b.polls.is_empty());
        b.feed(GGA);
        assert_eq!(b.polls, [Poll::Version, Poll::Rf]);

        b.feed(&frame(0x0a, 0x38, &rf_payload(1, 4, 0)));
        assert_eq!(
            b.r.info().rf,
            Some(Rf {
                jamming: Jamming::Ok,
                antenna_status: AntennaStatus::Open,
                antenna_power: AntennaPower::Off,
                noise: 87,
                agc: 4321,
                jamming_indicator: 12,
            })
        );
        b.feed(&frame(0x0a, 0x38, &[0u8; 8]));
        assert!(b.r.info().rf.is_some());
    }

    #[test]
    fn the_identity_is_asked_for_five_times_at_most() {
        let mut b = Bench::open();
        b.feed(GGA);
        b.advance(Duration::from_secs(9));
        b.feed(GGA);
        b.advance(Duration::from_secs(20));
        let versions = b.polls.iter().filter(|p| **p == Poll::Version).count();
        assert_eq!(versions, 5);
    }

    #[test]
    fn the_identity_stops_the_polls_and_lands_in_the_info() {
        let mut b = Bench::open();
        let payload = [
            field("ROM CORE 4.04 (d964f4)", 30),
            field("00190000", 10),
            field("FWVER=SPG 4.04", 30),
            field("PROTVER=32.01", 30),
        ];
        b.feed(&frame(0x0a, 0x04, &payload.concat()));
        let version = b.r.info().version.unwrap();
        assert_eq!(
            (version.firmware.as_deref(), version.protocol.as_deref()),
            (Some("SPG 4.04"), Some("32.01"))
        );
        let polls = b.polls.len();
        b.feed(GGA);
        b.advance(Duration::from_secs(5));
        assert!(!b.polls[polls..].contains(&Poll::Version));

        let mut b = Bench::open();
        b.feed(&frame(0x01, 0x07, &[]));
        b.feed(&frame(0x0a, 0x09, &[]));
        b.feed(&frame(0x0a, 0x04, &[0u8; 8]));
        assert_eq!((b.r.info().version, b.r.info().rf), (None, None));
        b.feed(&frame(0x0a, 0x04, &[0u8; 40]));
        assert_eq!(b.r.info().version.map(|v| v.software), Some(String::new()));
    }

    #[test]
    fn a_decoded_fix_goes_out_with_the_gst_accuracy() {
        let mut b = Bench::open();
        b.feed(&[GGA, RMC].concat());
        let fix = &b.fixes[0];
        assert!((fix.lat.unwrap() - 48.1173).abs() < 5e-4);
        assert!(fix.speed_ms.is_some() && fix.accuracy_m.is_none());

        b.feed(GST);
        assert_eq!(b.fixes.len(), 1);
        b.feed(GGA);
        assert!((b.fixes[1].accuracy_m.unwrap() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn satellites_dops_and_the_clock_are_kept_in_the_info() {
        let mut b = Bench::open();
        b.feed(&[GGA, GSA, RMC].concat());
        let info = b.r.info();
        assert_eq!(
            (info.fix_quality, info.fix_mode),
            (FixQuality::Gps, FixMode::ThreeD)
        );
        assert_eq!(
            (info.satellites_used, info.hdop, info.pdop),
            (8.0, Some(1.3), Some(2.5))
        );
        assert_eq!(info.receiver_time, Some(1_774_269_319_000.0));
        assert_eq!(info.updated_at, Some(1_800_000_000_000));
        assert_eq!(
            (info.device.as_deref(), info.baud_rate),
            (Some(DEFAULT_DEVICE), Some(38400))
        );
    }

    #[test]
    fn updates_are_paced_while_sentences_stream_in() {
        let mut b = Bench::open();
        b.feed(GGA);
        let before = b.infos.len();
        b.feed(GGA);
        b.feed(GGA);
        assert_eq!(b.infos.len(), before);
        b.advance(INFO_EVERY);
        assert_eq!(b.infos.len(), before + 1);
    }

    #[test]
    fn silence_drops_the_fix_state_and_what_the_module_said() {
        let mut b = Bench::open();
        b.feed(&[GGA, GSA].concat());
        b.feed(&version_frame("SPG 4.04"));
        b.feed(&frame(0x0a, 0x38, &rf_payload(1, 2, 1)));
        assert_eq!(b.r.info().fix_quality, FixQuality::Gps);
        b.advance(STALE);
        let info = b.r.info();
        assert_eq!(
            (info.fix_quality, info.fix_mode, info.satellites_used),
            (FixQuality::None, FixMode::None, 0.0)
        );
        assert_eq!((info.version, info.rf), (None, None));

        let mut b = Bench::open();
        let before = b.infos.len();
        b.advance(STALE);
        assert_eq!(b.infos.len(), before);
    }

    #[test]
    fn unplugged_and_plugged_back_the_identity_is_asked_for_again() {
        let mut b = Bench::open();
        b.feed(GGA);
        b.feed(&version_frame("SPG 4.04"));
        assert_eq!(
            b.r.info().version.and_then(|v| v.firmware).as_deref(),
            Some("SPG 4.04")
        );
        b.advance(STALE);
        assert!(!b.r.info().connected);
        b.polls.clear();
        b.feed(GGA);
        assert!(b.r.info().connected);
        assert_eq!(b.polls[0], Poll::Version);
        b.feed(&version_frame("SPG 5.10"));
        assert_eq!(
            b.r.info().version.and_then(|v| v.firmware).as_deref(),
            Some("SPG 5.10")
        );
    }

    #[test]
    fn the_rf_front_end_is_read_again_while_the_receiver_talks() {
        let mut b = Bench::open();
        b.feed(GGA);
        b.advance(Duration::from_secs(9));
        b.feed(GGA);
        b.advance(Duration::from_secs(2));
        assert_eq!(b.polls.iter().filter(|p| **p == Poll::Rf).count(), 2);

        b.r.stop();
        b.collect();
        let polls = b.polls.len();
        b.advance(Duration::from_secs(30));
        assert_eq!(b.polls.len(), polls);
    }

    #[test]
    fn a_failing_port_is_retried_and_reported_once() {
        let mut b = Bench::new();
        b.r.start();
        b.collect();
        b.r.failed(format!("{DEFAULT_DEVICE} not found"), b.now);
        b.collect();
        assert!(b.r.info().error.unwrap().contains("not found"));
        assert!(!b.r.info().connected);
        b.r.failed("again".into(), b.now);
        b.advance(RETRY);
        assert_eq!(b.opens, 2);
        b.r.opened(b.now);
        assert_eq!(b.r.info().error, None);
    }

    #[test]
    fn a_broken_port_disconnects_and_reopens() {
        let mut b = Bench::open();
        b.feed(GGA);
        b.r.failed("read failed: EIO".into(), b.now);
        b.collect();
        let info = b.r.info();
        assert!(!info.connected && info.error.unwrap().contains("EIO"));
        assert_eq!(b.closes, 1);
        b.advance(RETRY);
        assert_eq!(b.opens, 2);
    }

    #[test]
    fn start_and_stop() {
        let mut b = Bench::open();
        b.r.start();
        b.collect();
        assert_eq!(b.opens, 1);
        b.r.opened(b.now);
        b.collect();
        assert_eq!(b.infos.len(), 1);

        b.feed(GGA);
        b.r.stop();
        b.collect();
        assert_eq!(b.closes, 1);
        assert!(!b.infos.last().unwrap().connected);
        assert_eq!(b.r.next_deadline(), None);
        b.r.opened(b.now);
        assert!(!b.r.info().connected);

        let mut quiet = Bench::new();
        quiet.r.stop();
        quiet.collect();
        assert!(quiet.infos.is_empty());
    }
}
