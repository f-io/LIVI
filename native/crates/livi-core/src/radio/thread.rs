// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use livi_core_proto::state::{DabScan, DabSignal, DabStation, FmStation};
use livi_dab::{CHANNELS, Event, Receiver};
use livi_fm::Rds;
use livi_media::output::{AudioCodec, aac_raw_key};
use livi_rtlsdr::RtlSdr;
use tokio::sync::mpsc;

use super::audio::Format;
use super::{Band, Tuning, dab, fm};

const DAB_BLOCK: usize = 256 * 1024;
/// About 17 ms of FM, so the audio leaves in small, even steps.
const FM_BLOCK: usize = 64 * 1024;
const BUFFERS: usize = 16;
const READ_TIMEOUT: Duration = Duration::from_secs(1);
const LOOK_FOR_STICK: Duration = Duration::from_secs(3);
const DAB_SETTLE_BLOCKS: usize = 2;
const FM_SETTLE_BLOCKS: usize = 3;
const SCAN_SYNC: Duration = Duration::from_millis(2000);
const SCAN_LABELS: Duration = Duration::from_secs(8);
const SCAN_STABLE: Duration = Duration::from_millis(1200);

pub enum Cmd {
    Scan(Band),
    Play { frequency: u32, service: u32 },
    PlayFm { khz: u32 },
    SeekFm { from: u32, up: bool },
    Stop,
    Tuning(Tuning),
}

#[derive(Debug)]
pub enum Radio {
    Receiver(Option<String>),
    Progress(Band, DabScan),
    DabFound(Vec<DabStation>),
    FmFound {
        station: FmStation,
        noise_db: f32,
    },
    ScanDone(Band),
    FmSeeking(u32),
    FmTuned(u32),
    DabSignal(DabSignal),
    FmSignal(livi_fm::Signal),
    Format(Band, Format),
    Audio(Vec<Vec<u8>>),
    DabText(String),
    Rds(Rds),
    Logo {
        service: u32,
        mime: &'static str,
        data: Vec<u8>,
    },
    Error(String),
}

enum Mode {
    Idle,
    Scan {
        index: usize,
        since: Instant,
        synced: Option<Instant>,
        last_change: Instant,
    },
    Play {
        service: u32,
        frequency: u32,
    },
    FmScan {
        index: usize,
        probe: fm::Probe,
    },
    /// Settles on the first station up or down from `from`, or back on it.
    FmSeek {
        khz: u32,
        up: bool,
        from: u32,
        probe: fm::Probe,
    },
    FmPlay {
        khz: u32,
    },
}

struct Agc {
    gains: Vec<i32>,
    at: usize,
    power: f64,
    clipped: usize,
    samples: usize,
}

const AGC_WINDOW: usize = 256 * 1024;
const AGC_HIGH_DBFS: f64 = -10.0;
const AGC_LOW_DBFS: f64 = -22.0;
const AGC_CLIPPED: f64 = 0.001;
const AGC_START_TENTHS: i32 = 197;

impl Agc {
    fn new() -> Self {
        let gains = livi_rtlsdr::gains();
        let at = gains
            .iter()
            .position(|&g| g >= AGC_START_TENTHS)
            .unwrap_or(0);
        Self {
            gains,
            at,
            power: 0.0,
            clipped: 0,
            samples: 0,
        }
    }

    fn gain(&self) -> i32 {
        self.gains[self.at]
    }

    fn feed(&mut self, iq: &[u8]) -> Option<i32> {
        for p in iq.as_chunks::<2>().0 {
            if p.iter().any(|&v| v == 0 || v == 255) {
                self.clipped += 1;
            }
            let (i, q) = ((p[0] as f64 - 127.5) / 127.5, (p[1] as f64 - 127.5) / 127.5);
            self.power += i * i + q * q;
            self.samples += 1;
        }
        if self.samples < AGC_WINDOW {
            return None;
        }
        let dbfs = 10.0 * (self.power / self.samples as f64 / 2.0).max(1e-12).log10();
        let clipped = self.clipped as f64 / self.samples as f64;
        (self.power, self.clipped, self.samples) = (0.0, 0, 0);
        if (clipped > AGC_CLIPPED || dbfs > AGC_HIGH_DBFS) && self.at > 0 {
            self.at -= 1;
        } else if clipped == 0.0 && dbfs < AGC_LOW_DBFS && self.at + 1 < self.gains.len() {
            self.at += 1;
        } else {
            return None;
        }
        Some(self.gain())
    }
}

fn sample_rate(band: Band) -> u32 {
    match band {
        Band::Dab => livi_dab::tables::SAMPLE_RATE,
        Band::Fm => livi_fm::SAMPLE_RATE,
    }
}

struct Thread {
    events: mpsc::UnboundedSender<Radio>,
    sdr: Option<RtlSdr>,
    band: Band,
    dab: Receiver,
    fm: livi_fm::Receiver,
    fm_announced: bool,
    mode: Mode,
    tuning: Tuning,
    agc: Agc,
    settle: usize,
    looked: Option<Instant>,
}

pub fn run(cmds: std_mpsc::Receiver<Cmd>, events: mpsc::UnboundedSender<Radio>, tuning: Tuning) {
    let mut t = Thread {
        events,
        sdr: None,
        band: Band::Dab,
        dab: Receiver::new(),
        fm: livi_fm::Receiver::new(tuning.deemphasis_us),
        fm_announced: false,
        mode: Mode::Idle,
        tuning,
        agc: Agc::new(),
        settle: 0,
        looked: None,
    };
    let mut stream = None;
    loop {
        let wait = if matches!(t.mode, Mode::Idle) {
            Duration::from_millis(500)
        } else {
            Duration::ZERO
        };
        match cmds.recv_timeout(wait) {
            Ok(cmd) => {
                if t.command(cmd) {
                    stream = None;
                }
                continue;
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
        }
        if t.sdr.is_none() {
            stream = None;
            t.look_for_stick();
            if t.sdr.is_none() {
                if !matches!(t.mode, Mode::Idle) {
                    std::thread::sleep(Duration::from_millis(500));
                }
                continue;
            }
            if let Err(e) = t.retune() {
                t.lost(e);
                continue;
            }
        }
        if matches!(t.mode, Mode::Idle) {
            stream = None;
            t.check_stick();
            continue;
        }
        if stream.is_none() {
            let block = match t.band {
                Band::Dab => DAB_BLOCK,
                Band::Fm => FM_BLOCK,
            };
            match t.sdr.as_ref().map(|s| s.stream(BUFFERS, block)) {
                Some(Ok(s)) => stream = Some(s),
                Some(Err(e)) => {
                    t.lost(e.to_string());
                    continue;
                }
                None => continue,
            }
        }
        let Some(s) = stream.as_mut() else { continue };
        let block = match s.next_block(READ_TIMEOUT) {
            Ok(b) => b.to_vec(),
            Err(e) => {
                stream = None;
                t.lost(e.to_string());
                continue;
            }
        };
        if t.tuning.gain.is_none()
            && let Some(gain) = t.agc.feed(&block)
        {
            if let Some(sdr) = t.sdr.as_mut()
                && let Err(e) = sdr.set_gain(gain)
            {
                eprintln!("[radio] gain: {e}");
            }
            t.gain_moved();
        }
        if t.settle > 0 {
            t.settle -= 1;
            continue;
        }
        let restart = match t.band {
            Band::Dab => {
                let evs = t.dab.push_u8(&block);
                t.dab_step(evs)
            }
            Band::Fm => {
                let evs = t.fm.push_u8(&block);
                t.fm_step(evs)
            }
        };
        if restart {
            stream = None;
        }
    }
}

impl Thread {
    fn send(&self, e: Radio) {
        let _ = self.events.send(e);
    }

    fn look_for_stick(&mut self) {
        if self.looked.is_some_and(|at| at.elapsed() < LOOK_FOR_STICK) {
            return;
        }
        self.looked = Some(Instant::now());
        let Ok(list) = livi_rtlsdr::devices() else {
            return;
        };
        let Some(info) = list.first() else { return };
        let name = info.product_string().unwrap_or("RTL2832U").to_string();
        match RtlSdr::open(info) {
            Ok(sdr) => {
                println!("[radio] receiver {name} ready");
                self.sdr = Some(sdr);
                self.send(Radio::Receiver(Some(name)));
            }
            Err(e) => eprintln!("[radio] cannot open {name}: {e}"),
        }
    }

    /// The core stops everything on a lost stick, so nothing resumes on its own.
    fn lost(&mut self, why: String) {
        if self.sdr.take().is_some() {
            println!("[radio] receiver gone: {why}");
            self.send(Radio::Receiver(None));
        }
        self.mode = Mode::Idle;
        self.dab.select(None);
        self.looked = None;
    }

    /// Only transfers notice a pulled stick, and an idle one makes none.
    fn check_stick(&mut self) {
        if self.looked.is_some_and(|at| at.elapsed() < LOOK_FOR_STICK) {
            return;
        }
        self.looked = Some(Instant::now());
        if let Some(Err(e)) = self.sdr.as_ref().map(RtlSdr::check) {
            self.lost(e.to_string());
        }
    }

    /// Blocks still in flight carry the old gain.
    fn gain_moved(&mut self) {
        let probe = match &mut self.mode {
            Mode::FmScan { probe, .. } | Mode::FmSeek { probe, .. } => probe,
            _ => return,
        };
        if probe.restart() {
            self.fm.reset();
            self.settle = FM_SETTLE_BLOCKS;
        }
    }

    fn apply_tuning(&mut self) -> Result<(), livi_rtlsdr::Error> {
        self.fm.set_deemphasis(self.tuning.deemphasis_us);
        let Some(sdr) = self.sdr.as_mut() else {
            return Ok(());
        };
        sdr.set_ppm(self.tuning.ppm)?;
        sdr.set_sample_rate(sample_rate(self.band))?;
        sdr.set_gain(self.tuning.gain.unwrap_or(self.agc.gain()))
    }

    /// The stream restarts on a new band, its blocks are sized for it.
    fn use_band(&mut self, band: Band) -> bool {
        if self.band == band {
            return false;
        }
        self.band = band;
        if let Some(sdr) = self.sdr.as_mut()
            && let Err(e) = sdr.set_sample_rate(sample_rate(band))
        {
            self.send(Radio::Error(format!("sample rate: {e}")));
        }
        true
    }

    fn command(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Stop => {
                self.mode = Mode::Idle;
                self.dab.select(None);
                true
            }
            Cmd::Tuning(tuning) => {
                self.tuning = tuning;
                if let Err(e) = self.apply_tuning() {
                    eprintln!("[radio] tuning: {e}");
                }
                false
            }
            Cmd::Scan(band) => {
                let restart = self.use_band(band);
                match band {
                    Band::Dab => {
                        let now = Instant::now();
                        self.mode = Mode::Scan {
                            index: 0,
                            since: now,
                            synced: None,
                            last_change: now,
                        };
                        self.dab.select(None);
                        self.start_channel();
                    }
                    Band::Fm => {
                        self.mode = Mode::FmScan {
                            index: 0,
                            probe: fm::Probe::default(),
                        };
                        self.fm.set_audio(false);
                        self.start_frequency();
                    }
                }
                restart
            }
            Cmd::Play { frequency, service } => {
                let restart = self.use_band(Band::Dab);
                self.mode = Mode::Play { service, frequency };
                self.dab.select(Some(service));
                self.tune(frequency);
                restart
            }
            Cmd::SeekFm { from, up } => {
                let restart = self.use_band(Band::Fm);
                self.fm.set_audio(false);
                self.seek_to(fm::seek_next(from, up), up, from);
                restart
            }
            Cmd::PlayFm { khz } => {
                let restart = self.use_band(Band::Fm);
                self.mode = Mode::FmPlay { khz };
                self.fm.set_audio(true);
                self.fm_announced = false;
                self.tune(khz * 1000);
                restart
            }
        }
    }

    fn tune(&mut self, frequency: u32) {
        self.settle = match self.band {
            Band::Dab => {
                self.dab.reset();
                DAB_SETTLE_BLOCKS
            }
            Band::Fm => {
                self.fm.reset();
                FM_SETTLE_BLOCKS
            }
        };
        if let Some(sdr) = self.sdr.as_mut()
            && let Err(e) = sdr.set_center_freq(frequency)
        {
            self.send(Radio::Error(format!("tune {frequency} Hz: {e}")));
        }
    }

    fn retune(&mut self) -> Result<(), String> {
        self.apply_tuning().map_err(|e| e.to_string())?;
        match self.mode {
            Mode::Scan { .. } => self.start_channel(),
            Mode::Play { service, frequency } => {
                self.dab.select(Some(service));
                self.tune(frequency);
            }
            Mode::FmScan { .. } => self.start_frequency(),
            Mode::FmSeek { khz, up, from, .. } => self.seek_to(khz, up, from),
            Mode::FmPlay { khz } => self.tune(khz * 1000),
            Mode::Idle => {}
        }
        Ok(())
    }

    fn start_channel(&mut self) {
        let Mode::Scan { index, .. } = self.mode else {
            return;
        };
        let (name, freq) = CHANNELS[index];
        self.send(Radio::Progress(
            Band::Dab,
            DabScan {
                channel: name.to_string(),
                done: index as u32,
                total: CHANNELS.len() as u32,
                found: 0,
            },
        ));
        let now = Instant::now();
        self.mode = Mode::Scan {
            index,
            since: now,
            synced: None,
            last_change: now,
        };
        self.tune(freq);
    }

    fn start_frequency(&mut self) {
        let Mode::FmScan { index, .. } = self.mode else {
            return;
        };
        let khz = fm::khz_at(index);
        self.send(Radio::Progress(
            Band::Fm,
            DabScan {
                channel: fm::mhz(khz),
                done: index as u32,
                total: fm::count() as u32,
                found: 0,
            },
        ));
        self.mode = Mode::FmScan {
            index,
            probe: fm::Probe::default(),
        };
        self.tune(khz * 1000);
    }

    fn dab_step(&mut self, evs: Vec<Event>) -> bool {
        let mut ensemble_changed = false;
        for e in evs {
            match e {
                Event::Sync { locked, fic_ok } => {
                    if matches!(self.mode, Mode::Play { .. }) {
                        self.send(Radio::DabSignal(DabSignal {
                            locked,
                            quality: fic_ok as u32,
                        }));
                    }
                }
                Event::Ensemble(_) => ensemble_changed = true,
                Event::Format { format, asc } => self.send(Radio::Format(
                    Band::Dab,
                    Format {
                        codec: AudioCodec::AacRaw,
                        key: aac_raw_key(&asc),
                        rate: format.sample_rate(),
                        channels: format.output_channels() as u8,
                    },
                )),
                Event::Audio(units) => self.send(Radio::Audio(units)),
                Event::Label(t) => self.send(Radio::DabText(t)),
                Event::Logo(l) => self.send(Radio::Logo {
                    service: l.service,
                    mime: l.mime,
                    data: l.data,
                }),
            }
        }
        let Mode::Scan {
            index,
            since,
            synced,
            last_change,
        } = self.mode
        else {
            return false;
        };
        let now = Instant::now();
        let found = self.dab.locked() && self.dab.ensemble().id.is_some();
        let synced = synced.or_else(|| found.then_some(now));
        let last_change = if ensemble_changed { now } else { last_change };
        let complete = dab::labels_complete(self.dab.ensemble());
        let next = match synced {
            None => since.elapsed() > SCAN_SYNC,
            Some(at) => {
                (complete && last_change.elapsed() > SCAN_STABLE) || at.elapsed() > SCAN_LABELS
            }
        };
        self.mode = Mode::Scan {
            index,
            since,
            synced,
            last_change,
        };
        if !next {
            return false;
        }
        if synced.is_some() {
            let (name, freq) = CHANNELS[index];
            let list = dab::stations(self.dab.ensemble(), name, freq);
            if !list.is_empty() {
                println!("[radio] {name}: {} stations", list.len());
                self.send(Radio::DabFound(list));
            }
        }
        if index + 1 >= CHANNELS.len() {
            self.mode = Mode::Idle;
            self.send(Radio::ScanDone(Band::Dab));
            return true;
        }
        self.mode = Mode::Scan {
            index: index + 1,
            since: now,
            synced: None,
            last_change: now,
        };
        self.start_channel();
        false
    }

    fn fm_step(&mut self, evs: Vec<livi_fm::Event>) -> bool {
        let playing = matches!(self.mode, Mode::FmPlay { .. });
        for e in evs {
            match e {
                livi_fm::Event::Audio(pcm) if playing => {
                    if !std::mem::replace(&mut self.fm_announced, true) {
                        self.send(Radio::Format(
                            Band::Fm,
                            Format {
                                codec: AudioCodec::PcmLe,
                                key: [0; 32],
                                rate: livi_fm::AUDIO_RATE,
                                channels: 2,
                            },
                        ));
                    }
                    let bytes = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
                    self.send(Radio::Audio(vec![bytes]));
                }
                livi_fm::Event::Signal(s) if playing => self.send(Radio::FmSignal(s)),
                livi_fm::Event::Rds(r) if playing => self.send(Radio::Rds(r)),
                _ => {}
            }
        }
        match self.mode {
            Mode::FmScan { index, probe } => self.scan_step(index, probe),
            Mode::FmSeek {
                khz,
                up,
                from,
                probe,
            } => self.seek_step(khz, up, from, probe),
            _ => false,
        }
    }

    fn listen(&mut self, probe: &mut fm::Probe) -> fm::Verdict {
        probe.step(&self.fm.measured(), self.fm.rds_synced(), Instant::now())
    }

    fn scan_step(&mut self, index: usize, mut probe: fm::Probe) -> bool {
        let khz = fm::khz_at(index);
        match self.listen(&mut probe) {
            fm::Verdict::Wait => {
                self.mode = Mode::FmScan { index, probe };
                return false;
            }
            fm::Verdict::Station => {
                let station = FmStation {
                    khz,
                    name: self.fm.steady_name().unwrap_or("").trim().to_string(),
                    pi: self.fm.pi(),
                    favorite: false,
                    hidden: false,
                };
                let noise_db = self.fm.measured().noise_db;
                println!(
                    "[radio] {} MHz: {:?} {noise_db:.0} dB",
                    fm::mhz(khz),
                    station.name
                );
                self.send(Radio::FmFound { station, noise_db });
            }
            fm::Verdict::Nothing => {}
        }
        if index + 1 >= fm::count() {
            self.mode = Mode::Idle;
            self.send(Radio::ScanDone(Band::Fm));
            return true;
        }
        self.mode = Mode::FmScan {
            index: index + 1,
            probe: fm::Probe::default(),
        };
        self.start_frequency();
        false
    }

    fn seek_step(&mut self, khz: u32, up: bool, from: u32, mut probe: fm::Probe) -> bool {
        match self.listen(&mut probe) {
            fm::Verdict::Wait if probe.shows_station() => self.settle_on(khz),
            fm::Verdict::Wait => {
                self.mode = Mode::FmSeek {
                    khz,
                    up,
                    from,
                    probe,
                };
            }
            fm::Verdict::Station => self.settle_on(khz),
            fm::Verdict::Nothing => {
                let next = fm::seek_next(khz, up);
                if next == from {
                    self.tune(from * 1000);
                    self.settle_on(from);
                } else {
                    self.seek_to(next, up, from);
                }
            }
        }
        false
    }

    fn seek_to(&mut self, khz: u32, up: bool, from: u32) {
        self.mode = Mode::FmSeek {
            khz,
            up,
            from,
            probe: fm::Probe::default(),
        };
        self.send(Radio::FmSeeking(khz));
        self.tune(khz * 1000);
    }

    /// The seek heard the station already, its name goes on from there.
    fn settle_on(&mut self, khz: u32) {
        self.mode = Mode::FmPlay { khz };
        self.fm.set_audio(true);
        self.fm_announced = false;
        self.send(Radio::FmTuned(khz));
        if let Some(name) = self.fm.name() {
            self.send(Radio::Rds(Rds::Name(name.to_string())));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_gain_follows_level_and_clipping() {
        let mut agc = Agc::new();
        let start = agc.gain();
        assert_eq!(start, 197);
        let window = |i: u8, q: u8| [i, q].repeat(AGC_WINDOW);
        assert_eq!(agc.feed(&[0, 255]), None, "decides once a window is full");
        let mut fresh = Agc::new();
        assert!(
            fresh.feed(&window(0, 128)).unwrap() < start,
            "clipping steps down"
        );
        let mut loud = Agc::new();
        assert!(
            loud.feed(&window(20, 235)).unwrap() < start,
            "too loud steps down"
        );
        let mut quiet = Agc::new();
        assert!(
            quiet.feed(&window(128, 129)).unwrap() > start,
            "too quiet steps up"
        );
        let mut right = Agc::new();
        assert_eq!(right.feed(&window(100, 155)), None, "in range stays");
    }
}
