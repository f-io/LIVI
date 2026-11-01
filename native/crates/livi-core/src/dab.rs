// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use base64::Engine;
use livi_core_proto::message::MediaControl;
use livi_core_proto::state::{Dab, DabScan, DabSignal, DabStation, State};
use livi_dab::{CHANNELS, Ensemble, Event, Receiver};
use livi_media::gst_host::{AudioCodec, AudioOpts, GstHost, aac_raw_key};
use livi_rtlsdr::RtlSdr;
use tokio::sync::{mpsc, watch};

use crate::hub::Hub;
use crate::state_files::{Sdr, load_sdr, save_sdr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DabAsk {
    Scan,
    StopScan,
    Play(u32),
    Resume,
    Pause,
    Stop,
    Step(i32),
    Favorite { service: u32, favorite: bool },
    Hide { service: u32, hidden: bool },
    Move { service: u32, by: i32 },
}

fn merged(own: &[DabStation], mut found: Vec<DabStation>) -> Vec<DabStation> {
    let mut out: Vec<DabStation> = own
        .iter()
        .filter_map(|old| {
            let at = found.iter().position(|f| f.service_id == old.service_id)?;
            let mut st = found.remove(at);
            st.favorite = old.favorite;
            st.hidden = old.hidden;
            Some(st)
        })
        .collect();
    found.sort_by_key(|s| s.label.to_lowercase());
    out.extend(found);
    out
}

pub fn media_ask(dab: &Dab, phone_in_front: bool, control: MediaControl) -> Option<DabAsk> {
    let idle = !phone_in_front && !dab.stations.is_empty();
    if !dab.active && !idle {
        return None;
    }
    Some(match control {
        MediaControl::Next => DabAsk::Step(1),
        MediaControl::Prev => DabAsk::Step(-1),
        MediaControl::Pause => DabAsk::Pause,
        MediaControl::PlayPause if dab.playing => DabAsk::Pause,
        MediaControl::PlayPause | MediaControl::Play => DabAsk::Resume,
    })
}

fn step_target(list: &[DabStation], current: Option<u32>, by: i32) -> Option<u32> {
    let visible: Vec<&DabStation> = list.iter().filter(|s| !s.hidden).collect();
    if visible.is_empty() {
        return None;
    }
    let n = visible.len() as i64;
    if let Some(at) = visible.iter().position(|s| Some(s.service_id) == current) {
        return Some(visible[(at as i64 + by as i64).rem_euclid(n) as usize].service_id);
    }
    let at = current.and_then(|id| list.iter().position(|s| s.service_id == id));
    let Some(at) = at else {
        return Some(visible[0].service_id);
    };
    let len = list.len() as i64;
    let dir = if by < 0 { -1 } else { 1 };
    (1..=len)
        .map(|k| &list[(at as i64 + dir * k).rem_euclid(len) as usize])
        .find(|s| !s.hidden)
        .map(|s| s.service_id)
}

fn move_station(list: &mut [DabStation], service: u32, by: i32) -> bool {
    let Some(at) = list.iter().position(|s| s.service_id == service) else {
        return false;
    };
    let to = (at as i64 + by as i64).clamp(0, list.len() as i64 - 1) as usize;
    if to == at {
        return false;
    }
    if to > at {
        list[at..=to].rotate_left(1);
    } else {
        list[to..=at].rotate_right(1);
    }
    true
}

fn star(list: &mut Vec<DabStation>, service: u32, favorite: bool) -> bool {
    let Some(at) = list.iter().position(|s| s.service_id == service) else {
        return false;
    };
    let mut st = list.remove(at);
    st.favorite = favorite;
    let to = list.iter().rposition(|s| s.favorite).map_or(0, |i| i + 1);
    list.insert(to, st);
    true
}

fn hidden_last(list: &mut [DabStation]) {
    list.sort_by_key(|s| s.hidden);
}

pub type DabAsks = mpsc::UnboundedReceiver<DabAsk>;

const BLOCK: usize = 256 * 1024;
const BUFFERS: usize = 16;
const READ_TIMEOUT: Duration = Duration::from_secs(1);
const LOOK_FOR_STICK: Duration = Duration::from_secs(3);
const SETTLE_BLOCKS: usize = 2;
const SCAN_SYNC: Duration = Duration::from_millis(2000);
const SCAN_LABELS: Duration = Duration::from_secs(8);
const SCAN_STABLE: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Tuning {
    gain: Option<i32>,
    ppm: i32,
}

fn tuning(state: &State) -> Tuning {
    let c = &state.config;
    let tenths = (c.dab_gain_db * 10.0).round() as i32;
    let gain = (!c.dab_gain_auto).then(|| nearest_gain(tenths));
    Tuning {
        gain,
        ppm: c.dab_ppm,
    }
}

fn nearest_gain(tenths: i32) -> i32 {
    livi_rtlsdr::gains()
        .into_iter()
        .min_by_key(|g| (g - tenths).abs())
        .unwrap_or(0)
}

enum Cmd {
    Scan(Tuning),
    Play {
        frequency: u32,
        service: u32,
        tuning: Tuning,
    },
    Stop,
    Tuning(Tuning),
}

#[derive(Debug)]
enum Radio {
    Receiver(Option<String>),
    Progress(DabScan),
    Found(Vec<DabStation>),
    ScanDone,
    Signal(DabSignal),
    Format {
        asc: Vec<u8>,
        rate: u32,
        channels: u8,
    },
    Units(Vec<Vec<u8>>),
    Text(String),
    Logo {
        service: u32,
        mime: &'static str,
        data: Vec<u8>,
    },
    Error(String),
}

pub async fn run(
    hub: Arc<Hub>,
    gst: GstHost,
    mut asks: DabAsks,
    user_data: PathBuf,
    mut resume: bool,
) {
    let (cmd_tx, cmd_rx) = std_mpsc::channel();
    let (ev_tx, mut events) = mpsc::unbounded_channel();
    let mut out = Out {
        hub: hub.clone(),
        gst: gst.clone(),
        cmds: cmd_tx.clone(),
        user_data: user_data.clone(),
        own: load_sdr(&user_data),
        logos: load_images(&user_data.join(LOGOS)),
    };
    hidden_last(&mut out.own.stations);
    let gains_db = livi_rtlsdr::gains()
        .into_iter()
        .map(|g| g as f64 / 10.0)
        .collect();
    hub.update(|s| s.dab.gains_db = gains_db);
    out.publish();
    let started = std::thread::Builder::new()
        .name("dab".into())
        .spawn(move || radio_thread(cmd_rx, ev_tx));
    if let Err(e) = started {
        eprintln!("[dab] no radio thread: {e}");
        return;
    }
    let mut state: watch::Receiver<State> = hub.watch();
    let mut tune = tuning(&state.borrow());
    let mut audio = Audio::default();
    let mut found: Vec<DabStation> = Vec::new();

    loop {
        tokio::select! {
            ask = asks.recv() => {
                let Some(ask) = ask else { return };
                let s = state.borrow().clone();
                match ask {
                    DabAsk::Scan => {
                        found.clear();
                        audio.close(&gst);
                        hub.update(|s| {
                            s.dab.station = None;
                            s.dab.playing = false;
                            s.dab.error = None;
                            s.dab.scan = Some(DabScan { total: CHANNELS.len() as u32, ..Default::default() });
                        });
                        let _ = cmd_tx.send(Cmd::Scan(tune));
                    }
                    DabAsk::StopScan | DabAsk::Stop => {
                        audio.close(&gst);
                        let _ = cmd_tx.send(Cmd::Stop);
                        hub.update(|s| {
                            s.dab.scan = None;
                            s.dab.active = false;
                            s.dab.station = None;
                            s.dab.playing = false;
                            s.dab.text = None;
                        });
                    }
                    DabAsk::Pause => {
                        audio.close(&gst);
                        let _ = cmd_tx.send(Cmd::Stop);
                        hub.update(|s| s.dab.playing = false);
                    }
                    DabAsk::Play(service) => out.play(&mut audio, &s, service, tune),
                    DabAsk::Resume => out.resume(&mut audio, &s, tune),
                    DabAsk::Favorite { service, favorite } => {
                        if star(&mut out.own.stations, service, favorite) {
                            out.keep();
                        }
                    }
                    DabAsk::Hide { service, hidden } => {
                        if let Some(st) = out.own.stations.iter_mut().find(|st| st.service_id == service) {
                            st.hidden = hidden;
                            out.keep();
                        }
                    }
                    DabAsk::Move { service, by } => {
                        if move_station(&mut out.own.stations, service, by) {
                            out.keep();
                        }
                    }
                    DabAsk::Step(by) => {
                        if let Some(service) = step_target(&s.dab.stations, s.dab.station, by) {
                            out.play(&mut audio, &s, service, tune);
                        }
                    }
                }
            }
            ev = events.recv() => {
                let Some(ev) = ev else { return };
                match ev {
                    Radio::Receiver(None) => {
                        audio.close(&gst);
                        found.clear();
                        let _ = cmd_tx.send(Cmd::Stop);
                        hub.update(|s| stick_gone(&mut s.dab));
                    }
                    Radio::Receiver(name) => {
                        hub.update(|s| s.dab.receiver = name);
                        if std::mem::take(&mut resume) {
                            let s = state.borrow().clone();
                            out.resume(&mut audio, &s, tune);
                        }
                    }
                    Radio::Progress(p) => {
                        let found = found.len() as u32;
                        hub.update(|s| s.dab.scan = Some(DabScan { found, ..p }));
                    }
                    Radio::Found(list) => {
                        for st in list {
                            if !found.iter().any(|f| f.service_id == st.service_id) {
                                found.push(st);
                            }
                        }
                        let n = found.len() as u32;
                        hub.update(|s| if let Some(p) = s.dab.scan.as_mut() { p.found = n });
                    }
                    Radio::ScanDone => {
                        out.own.stations = merged(&out.own.stations, std::mem::take(&mut found));
                        out.keep();
                        hub.update(|s| s.dab.scan = None);
                    }
                    Radio::Signal(sig) => hub.update(|s| s.dab.signal = sig),
                    Radio::Format { asc, rate, channels } => {
                        let s = state.borrow().clone();
                        audio.open(&gst, &s, &asc, rate, channels).await;
                        hub.update(|s| s.dab.playing = audio.stream.is_some());
                    }
                    Radio::Units(units) => audio.push(&gst, &units),
                    Radio::Text(t) => hub.update(|s| s.dab.text = Some(t)),
                    Radio::Logo { service, mime, data } => {
                        keep_image(&user_data.join(LOGOS), service, mime, &data);
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        if out.logos.insert(service, b64.clone()) != Some(b64) {
                            out.publish();
                        }
                    }
                    Radio::Error(e) => {
                        eprintln!("[dab] {e}");
                        audio.close(&gst);
                        hub.update(|s| {
                            s.dab.error = Some(e);
                            s.dab.playing = false;
                        });
                    }
                }
            }
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                let s = state.borrow_and_update().clone();
                let next = tuning(&s);
                if next != tune {
                    tune = next;
                    let _ = cmd_tx.send(Cmd::Tuning(tune));
                }
                audio.follow(&gst, &s);
            }
        }
    }
}

struct Out {
    hub: Arc<Hub>,
    gst: GstHost,
    cmds: std_mpsc::Sender<Cmd>,
    user_data: PathBuf,
    own: Sdr,
    logos: HashMap<u32, String>,
}

impl Out {
    fn publish(&self) {
        let own = &self.own;
        let mut stations = own.stations.clone();
        for st in &mut stations {
            st.logo = self.logos.get(&st.service_id).cloned();
        }
        self.hub.update(|s| {
            s.dab.stations = stations;
            s.dab.last_station = own.last_station;
        });
    }

    fn keep(&mut self) {
        hidden_last(&mut self.own.stations);
        save_sdr(&self.user_data, &self.own);
        self.publish();
    }

    fn resume(&mut self, audio: &mut Audio, s: &State, tuning: Tuning) {
        if s.dab.playing {
            return;
        }
        let list = &s.dab.stations;
        let wanted = s.dab.station.or(s.dab.last_station);
        let pick = list.iter().find(|st| Some(st.service_id) == wanted);
        if let Some(st) = pick.or(list.iter().find(|st| !st.hidden)) {
            self.play(audio, s, st.service_id, tuning);
        }
    }

    fn play(&mut self, audio: &mut Audio, s: &State, service: u32, tuning: Tuning) {
        let Some(st) = s.dab.stations.iter().find(|st| st.service_id == service) else {
            self.hub
                .update(|s| s.dab.error = Some("station not in the list, scan again".into()));
            return;
        };
        audio.close(&self.gst);
        let _ = self.cmds.send(Cmd::Play {
            frequency: st.frequency_hz,
            service,
            tuning,
        });
        if self.own.last_station != Some(service) {
            self.own.last_station = Some(service);
            self.keep();
        }
        self.hub.update(|s| {
            s.dab.active = true;
            s.dab.scan = None;
            s.dab.station = Some(service);
            s.dab.playing = false;
            s.dab.text = None;
            s.dab.error = None;
        });
    }
}

fn stick_gone(dab: &mut Dab) {
    *dab = Dab {
        stations: std::mem::take(&mut dab.stations),
        last_station: dab.last_station,
        gains_db: std::mem::take(&mut dab.gains_db),
        ..Dab::default()
    };
}

const LOGOS: &str = "dab/logos";

fn image_path(dir: &Path, service: u32) -> Option<PathBuf> {
    ["jpg", "png"]
        .iter()
        .map(|ext| dir.join(format!("{service:08x}.{ext}")))
        .find(|p| p.exists())
}

fn keep_image(dir: &Path, service: u32, mime: &str, data: &[u8]) {
    let ext = if mime == "image/png" { "png" } else { "jpg" };
    if let Some(old) = image_path(dir, service) {
        let _ = std::fs::remove_file(old);
    }
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(dir.join(format!("{service:08x}.{ext}")), data);
    }
}

fn load_images(dir: &Path) -> HashMap<u32, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let service = u32::from_str_radix(path.file_stem()?.to_str()?, 16).ok()?;
            let data = std::fs::read(&path).ok()?;
            Some((
                service,
                base64::engine::general_purpose::STANDARD.encode(data),
            ))
        })
        .collect()
}

#[derive(Default)]
struct Audio {
    stream: Option<u32>,
    level: f64,
}

impl Audio {
    async fn open(&mut self, gst: &GstHost, s: &State, asc: &[u8], rate: u32, channels: u8) {
        self.close(gst);
        if s.config.disable_audio_output {
            return;
        }
        let device = s.config.audio_output_device.clone().unwrap_or_default();
        let opts = AudioOpts {
            codec: AudioCodec::AacRaw,
            payload_type: 0,
            clock_rate: rate,
            channels,
            latency_ms: 0,
            realtime: false,
            fed: true,
            device,
        };
        let (id, _, _) = gst.open_audio(&aac_raw_key(asc), &opts).await;
        if id == 0 {
            return;
        }
        gst.set_audio_active(id, true);
        gst.set_audio_volume(id, s.config.audio_volume, 0);
        self.stream = Some(id);
        self.level = s.config.audio_volume;
    }

    fn push(&self, gst: &GstHost, units: &[Vec<u8>]) {
        if let Some(id) = self.stream {
            for u in units {
                gst.push_audio(id, u);
            }
        }
    }

    fn follow(&mut self, gst: &GstHost, s: &State) {
        let Some(id) = self.stream else { return };
        if s.config.disable_audio_output {
            self.close(gst);
            return;
        }
        if s.config.audio_volume != self.level {
            self.level = s.config.audio_volume;
            gst.set_audio_volume(id, self.level, 100);
        }
    }

    fn close(&mut self, gst: &GstHost) {
        if let Some(id) = self.stream.take() {
            gst.close_audio(id);
        }
    }
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

struct Thread {
    events: mpsc::UnboundedSender<Radio>,
    sdr: Option<RtlSdr>,
    rx: Receiver,
    mode: Mode,
    tuning: Tuning,
    agc: Agc,
    settle: usize,
    looked: Option<Instant>,
}

fn radio_thread(cmds: std_mpsc::Receiver<Cmd>, events: mpsc::UnboundedSender<Radio>) {
    let mut t = Thread {
        events,
        sdr: None,
        rx: Receiver::new(),
        mode: Mode::Idle,
        tuning: Tuning { gain: None, ppm: 0 },
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
            continue;
        }
        if stream.is_none() {
            match t.sdr.as_ref().map(|s| s.stream(BUFFERS, BLOCK)) {
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
            && let Some(sdr) = t.sdr.as_mut()
            && let Err(e) = sdr.set_gain(gain)
        {
            eprintln!("[dab] gain: {e}");
        }
        if t.settle > 0 {
            t.settle -= 1;
            continue;
        }
        let evs = t.rx.push_u8(&block);
        if t.step(evs) {
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
            Ok(mut sdr) => {
                if let Err(e) = apply_tuning(&mut sdr, self.tuning, self.agc.gain()) {
                    eprintln!("[dab] tuning: {e}");
                }
                println!("[dab] receiver {name} ready");
                self.sdr = Some(sdr);
                self.send(Radio::Receiver(Some(name)));
            }
            Err(e) => eprintln!("[dab] cannot open {name}: {e}"),
        }
    }

    fn lost(&mut self, why: String) {
        if self.sdr.take().is_some() {
            println!("[dab] receiver gone: {why}");
            self.send(Radio::Receiver(None));
        }
        self.looked = None;
    }

    fn command(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Stop => {
                self.mode = Mode::Idle;
                self.rx.select(None);
                true
            }
            Cmd::Tuning(tuning) => {
                self.tuning = tuning;
                let auto = self.agc.gain();
                if let Some(sdr) = self.sdr.as_mut()
                    && let Err(e) = apply_tuning(sdr, tuning, auto)
                {
                    eprintln!("[dab] tuning: {e}");
                }
                false
            }
            Cmd::Scan(tuning) => {
                self.tuning = tuning;
                let now = Instant::now();
                self.mode = Mode::Scan {
                    index: 0,
                    since: now,
                    synced: None,
                    last_change: now,
                };
                self.rx.select(None);
                self.start_channel();
                false
            }
            Cmd::Play {
                frequency,
                service,
                tuning,
            } => {
                self.tuning = tuning;
                self.mode = Mode::Play { service, frequency };
                self.rx.select(Some(service));
                self.tune(frequency);
                false
            }
        }
    }

    fn tune(&mut self, frequency: u32) {
        self.rx.reset();
        self.settle = SETTLE_BLOCKS;
        if let Some(sdr) = self.sdr.as_mut()
            && let Err(e) = sdr.set_center_freq(frequency)
        {
            self.send(Radio::Error(format!("tune {frequency} Hz: {e}")));
        }
    }

    fn retune(&mut self) -> Result<(), String> {
        let (tuning, auto) = (self.tuning, self.agc.gain());
        if let Some(sdr) = self.sdr.as_mut() {
            apply_tuning(sdr, tuning, auto).map_err(|e| e.to_string())?;
        }
        match self.mode {
            Mode::Scan { .. } => self.start_channel(),
            Mode::Play { service, frequency } => {
                self.rx.select(Some(service));
                self.tune(frequency);
            }
            Mode::Idle => {}
        }
        Ok(())
    }

    fn start_channel(&mut self) {
        let Mode::Scan { index, .. } = self.mode else {
            return;
        };
        let (name, freq) = CHANNELS[index];
        self.send(Radio::Progress(DabScan {
            channel: name.to_string(),
            done: index as u32,
            total: CHANNELS.len() as u32,
            found: 0,
        }));
        let now = Instant::now();
        self.mode = Mode::Scan {
            index,
            since: now,
            synced: None,
            last_change: now,
        };
        self.tune(freq);
    }

    fn step(&mut self, evs: Vec<Event>) -> bool {
        let mut ensemble_changed = false;
        for e in evs {
            match e {
                Event::Sync { locked, fic_ok } => {
                    if matches!(self.mode, Mode::Play { .. }) {
                        self.send(Radio::Signal(DabSignal {
                            locked,
                            quality: fic_ok as u32,
                        }));
                    }
                }
                Event::Ensemble(_) => ensemble_changed = true,
                Event::Format { format, asc } => self.send(Radio::Format {
                    asc,
                    rate: format.sample_rate(),
                    channels: format.output_channels() as u8,
                }),
                Event::Audio(units) => self.send(Radio::Units(units)),
                Event::Label(t) => self.send(Radio::Text(t)),
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
        let found = self.rx.locked() && self.rx.ensemble().id.is_some();
        let synced = synced.or_else(|| found.then_some(now));
        let last_change = if ensemble_changed { now } else { last_change };
        let complete = labels_complete(self.rx.ensemble());
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
            let list = stations(self.rx.ensemble(), name, freq);
            if !list.is_empty() {
                println!("[dab] {name}: {} stations", list.len());
                self.send(Radio::Found(list));
            }
        }
        if index + 1 >= CHANNELS.len() {
            self.mode = Mode::Idle;
            self.send(Radio::ScanDone);
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
}

fn apply_tuning(sdr: &mut RtlSdr, t: Tuning, auto: i32) -> Result<(), livi_rtlsdr::Error> {
    sdr.set_ppm(t.ppm)?;
    sdr.set_gain(t.gain.unwrap_or(auto))
}

fn labels_complete(e: &Ensemble) -> bool {
    e.label.is_some()
        && e.dab_plus_services().count() > 0
        && e.dab_plus_services().all(|(s, _)| s.label.is_some())
}

fn stations(e: &Ensemble, channel: &str, frequency: u32) -> Vec<DabStation> {
    let ensemble = e.label.clone().unwrap_or_default();
    e.dab_plus_services()
        .filter_map(|(s, _)| {
            Some(DabStation {
                service_id: s.id,
                label: s.label.clone()?,
                short_label: s.short_label.clone().unwrap_or_default(),
                ensemble: ensemble.clone(),
                channel: channel.to_string(),
                frequency_hz: frequency,
                favorite: false,
                hidden: false,
                logo: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use livi_dab::fic::{Component, Protection, Service, Subchannel};

    fn ensemble() -> Ensemble {
        let mut e = Ensemble {
            label: Some("DR Deutschland".into()),
            ..Default::default()
        };
        e.subchannels.insert(
            11,
            Subchannel {
                id: 11,
                start: 616,
                size: 104,
                protection: Protection::Eep {
                    option: 0,
                    level: 1,
                },
            },
        );
        e.services.insert(
            0xd220,
            Service {
                id: 0xd220,
                label: Some("Dlf Kultur".into()),
                short_label: Some("Kultur".into()),
                components: vec![Component {
                    subchannel: 11,
                    ascty: 63,
                    primary: true,
                }],
                ..Default::default()
            },
        );
        e
    }

    #[test]
    fn a_scanned_ensemble_becomes_stations() {
        let list = stations(&ensemble(), "5C", 178_352_000);
        assert_eq!(
            list,
            vec![DabStation {
                service_id: 0xd220,
                label: "Dlf Kultur".into(),
                short_label: "Kultur".into(),
                ensemble: "DR Deutschland".into(),
                channel: "5C".into(),
                frequency_hz: 178_352_000,
                favorite: false,
                hidden: false,
                logo: None,
            }]
        );
        assert!(labels_complete(&ensemble()));
    }

    #[test]
    fn labels_are_incomplete_until_every_service_has_one() {
        let mut e = ensemble();
        e.services.get_mut(&0xd220).unwrap().label = None;
        assert!(!labels_complete(&e));
        assert!(!labels_complete(&Ensemble::default()));
    }

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

    #[test]
    fn manual_gain_snaps_to_a_tuner_step() {
        assert_eq!(nearest_gain(200), 197);
        assert_eq!(nearest_gain(-5), 0);
        assert_eq!(nearest_gain(900), 496);
    }

    fn named(id: u32, label: &str, favorite: bool) -> DabStation {
        DabStation {
            service_id: id,
            label: label.into(),
            favorite,
            hidden: false,
            logo: None,
            ..stations(&ensemble(), "5C", 1)[0].clone()
        }
    }

    fn ids(list: &[DabStation]) -> Vec<u32> {
        list.iter().map(|s| s.service_id).collect()
    }

    fn hidden(id: u32) -> DabStation {
        DabStation {
            hidden: true,
            ..named(id, "h", false)
        }
    }

    #[test]
    fn media_keys_steer_the_radio_while_it_is_the_source() {
        let mut dab = Dab::default();
        let mut phone = false;
        assert_eq!(
            media_ask(&dab, phone, MediaControl::Next),
            None,
            "no stations, the phone keeps the keys"
        );
        dab.stations = vec![named(1, "a", false)];
        assert_eq!(
            media_ask(&dab, phone, MediaControl::Next),
            Some(DabAsk::Step(1)),
            "nothing else plays"
        );
        phone = true;
        assert_eq!(
            media_ask(&dab, phone, MediaControl::Next),
            None,
            "a phone is in front"
        );
        dab.active = true;
        assert_eq!(
            media_ask(&dab, phone, MediaControl::Prev),
            Some(DabAsk::Step(-1))
        );
        assert_eq!(
            media_ask(&dab, phone, MediaControl::Pause),
            Some(DabAsk::Pause)
        );
        assert_eq!(
            media_ask(&dab, phone, MediaControl::PlayPause),
            Some(DabAsk::Resume)
        );
        assert_eq!(
            media_ask(&dab, phone, MediaControl::Play),
            Some(DabAsk::Resume)
        );
        dab.playing = true;
        assert_eq!(
            media_ask(&dab, phone, MediaControl::PlayPause),
            Some(DabAsk::Pause)
        );
    }

    #[test]
    fn a_pulled_stick_hands_the_player_back_to_the_phone() {
        let mut dab = Dab {
            receiver: Some("stick".into()),
            stations: vec![named(1, "a", true)],
            last_station: Some(1),
            active: true,
            station: Some(1),
            playing: true,
            text: Some("Lesart".into()),
            gains_db: vec![0.0, 0.9],
            ..Dab::default()
        };
        stick_gone(&mut dab);
        assert!(!dab.active && !dab.playing && dab.receiver.is_none());
        assert_eq!((dab.station, dab.text), (None, None));
        assert_eq!(dab.stations.len(), 1, "the list stays");
        assert_eq!(dab.last_station, Some(1));
        assert_eq!(dab.gains_db, [0.0, 0.9], "the gain setting keeps its steps");
    }

    #[test]
    fn a_star_moves_the_station_behind_the_last_favorite() {
        let mut list = vec![
            named(1, "a", true),
            named(2, "b", false),
            named(3, "c", false),
            named(4, "d", false),
        ];
        assert!(star(&mut list, 4, true));
        assert_eq!(ids(&list), [1, 4, 2, 3]);
        assert!(list[1].favorite);
        assert!(star(&mut list, 1, false));
        assert_eq!(
            ids(&list),
            [4, 1, 2, 3],
            "unstarred it follows the remaining favorites"
        );
        assert!(!list[1].favorite);
        assert!(star(&mut list, 4, false));
        assert_eq!(
            ids(&list),
            [4, 1, 2, 3],
            "no favorites left, it goes to the top"
        );
        assert!(!star(&mut list, 9, true));
    }

    #[test]
    fn hidden_stations_sink_to_the_end() {
        let mut list = vec![
            named(1, "a", true),
            hidden(2),
            named(3, "c", false),
            hidden(4),
        ];
        list.push(named(5, "e", false));
        hidden_last(&mut list);
        assert_eq!(ids(&list), [1, 3, 5, 2, 4]);
        list[3].hidden = false;
        hidden_last(&mut list);
        assert_eq!(
            ids(&list),
            [1, 3, 5, 2, 4],
            "shown again it closes the visible ones"
        );
        list[0].hidden = true;
        hidden_last(&mut list);
        assert_eq!(ids(&list), [3, 5, 2, 1, 4]);
    }

    #[test]
    fn steps_skip_hidden_stations() {
        let list = [
            named(1, "a", false),
            hidden(2),
            named(3, "c", false),
            hidden(4),
        ];
        assert_eq!(step_target(&list, Some(1), 1), Some(3));
        assert_eq!(
            step_target(&list, Some(3), 1),
            Some(1),
            "wraps past the hidden end"
        );
        assert_eq!(step_target(&list, Some(1), -1), Some(3));
        assert_eq!(
            step_target(&list, Some(2), 1),
            Some(3),
            "from a hidden one onwards"
        );
        assert_eq!(step_target(&list, Some(2), -1), Some(1));
        assert_eq!(step_target(&list, None, 1), Some(1));
        assert_eq!(step_target(&list, Some(9), 1), Some(1));
        assert_eq!(step_target(&[hidden(2)], Some(2), 1), None);
    }

    #[test]
    fn a_new_scan_keeps_own_order_and_favorites() {
        let own = [
            hidden(3),
            named(1, "a", true),
            DabStation {
                hidden: true,
                ..named(2, "b", false)
            },
        ];
        let found = vec![
            named(2, "b", false),
            named(1, "a new", false),
            named(5, "e", false),
            named(4, "d", false),
        ];
        let list = merged(&own, found);
        assert_eq!(
            ids(&list),
            [1, 2, 4, 5],
            "3 is gone, 4 and 5 come last by name"
        );
        assert!(list[0].favorite);
        assert_eq!(list[0].label, "a new");
        assert!(list[1].hidden, "hidden stays hidden");
        assert!(!list[2].hidden);
    }

    #[test]
    fn stations_move_within_the_own_order() {
        let mut list = vec![
            named(1, "a", false),
            named(2, "b", false),
            named(3, "c", false),
        ];
        assert!(move_station(&mut list, 1, 2));
        assert_eq!(ids(&list), [2, 3, 1]);
        assert!(move_station(&mut list, 1, -1));
        assert_eq!(ids(&list), [2, 1, 3]);
        assert!(!move_station(&mut list, 2, -1), "already at the top");
        assert!(!move_station(&mut list, 9, 1), "unknown station");
    }

    #[test]
    fn logos_are_kept_per_station() {
        let dir = std::env::temp_dir().join(format!("livi-dab-logos-{}", std::process::id()));
        keep_image(&dir, 0xd220, "image/jpeg", b"jpg");
        assert_eq!(image_path(&dir, 0xd220), Some(dir.join("0000d220.jpg")));
        keep_image(&dir, 0xd220, "image/png", b"png");
        assert_eq!(image_path(&dir, 0xd220), Some(dir.join("0000d220.png")));
        assert!(!dir.join("0000d220.jpg").exists());
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let loaded = load_images(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[&0xd220], "cG5n");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_images(&dir).is_empty());
    }
}
