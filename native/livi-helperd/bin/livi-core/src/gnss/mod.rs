mod clock;
mod nmea;
mod port;
mod receiver;
pub mod timezone;
mod tz_data;
mod ubx;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use livi_core_proto::config::Config;
use livi_core_proto::gnss as info;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::server::Core;
use clock::{Clock, Step};
use info::{GnssInfo, GpsFix};
use port::{Chunk, Port};
use receiver::{DEFAULT_BAUD_RATE, DEFAULT_DEVICE, Output, Poll, Receiver};

const SET_TIME_HELPER: &str = "/usr/local/lib/livi/livi-set-time.sh";

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

/// A no-break space counts as white space, a next line character does not.
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n', '\u{b}', '\u{c}', '\r', '\u{a0}'])
}

fn to_json(value: &impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[derive(Debug, Clone, Copy)]
struct Moment {
    at: Instant,
    unix_ms: u64,
}

impl Moment {
    fn now() -> Self {
        let unix_ms =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        Self { at: Instant::now(), unix_ms }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Settings {
    enabled: bool,
    device: String,
    baud_rate: u32,
}

impl Settings {
    fn of(config: &Config) -> Self {
        Self {
            enabled: config.gps_enabled,
            device: config.gps_device.clone(),
            baud_rate: config.gps_baud_rate,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Job {
    ApplyTimezone(String),
    SaveTimezone(String),
    StepClock(Step),
}

type Opener = Box<dyn FnMut(&str, u32) -> Result<Port, String> + Send>;

struct Gnss {
    telemetry: mpsc::UnboundedSender<Value>,
    open: Opener,
    clock: Clock,
    receiver: Option<Receiver>,
    port: Option<Port>,
    device: String,
    baud_rate: u32,
    timezone: Option<String>,
    /// The position rounded to about a kilometer.
    zone_key: String,
    jobs: Vec<Job>,
}

impl Gnss {
    fn new(telemetry: mpsc::UnboundedSender<Value>, open: Opener, clock: Clock) -> Self {
        Self {
            telemetry,
            open,
            clock,
            receiver: None,
            port: None,
            device: String::new(),
            baud_rate: 0,
            timezone: None,
            zone_key: String::new(),
            jobs: Vec::new(),
        }
    }

    fn begin(&mut self, config: &Config, now: Moment) {
        if !config.timezone.is_empty() {
            self.timezone = Some(config.timezone.clone());
            self.jobs.push(Job::ApplyTimezone(config.timezone.clone()));
        }
        self.apply(&Settings::of(config), now);
    }

    fn apply(&mut self, settings: &Settings, now: Moment) {
        if !settings.enabled {
            self.stop_receiver(now);
            return;
        }
        let device = if settings.device.is_empty() { DEFAULT_DEVICE } else { &settings.device };
        let baud_rate =
            if settings.baud_rate == 0 { DEFAULT_BAUD_RATE } else { settings.baud_rate };
        if self.receiver.is_some() && device == self.device && baud_rate == self.baud_rate {
            return;
        }
        self.stop_receiver(now);
        self.device = device.to_string();
        self.baud_rate = baud_rate;
        let mut receiver = Receiver::new(device, baud_rate);
        receiver.start();
        self.receiver = Some(receiver);
        self.pump(now);
    }

    fn stop_receiver(&mut self, now: Moment) {
        let Some(mut receiver) = self.receiver.take() else { return };
        receiver.stop();
        self.port = None;
        self.clock.release();
        let info = GnssInfo {
            device: Some(self.device.clone()),
            baud_rate: Some(self.baud_rate),
            ..Default::default()
        };
        self.on_info(info, now);
    }

    fn read(&mut self, chunk: Option<Chunk>, now: Moment) {
        let Some(receiver) = self.receiver.as_mut() else { return };
        match chunk {
            Some(Ok(bytes)) => receiver.data(&bytes, now.at, now.unix_ms),
            Some(Err(reason)) => receiver.failed(reason, now.at),
            None => receiver.failed(format!("{} closed", self.device), now.at),
        }
        self.pump(now);
    }

    fn tick(&mut self, now: Moment) {
        if let Some(receiver) = self.receiver.as_mut() {
            receiver.tick(now.at);
        }
        self.pump(now);
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.receiver.as_ref().and_then(Receiver::next_deadline)
    }

    async fn next_chunk(&mut self) -> Option<Chunk> {
        match self.port.as_mut() {
            Some(port) => port.chunks.recv().await,
            None => std::future::pending().await,
        }
    }

    fn take_jobs(&mut self) -> Vec<Job> {
        std::mem::take(&mut self.jobs)
    }

    fn pump(&mut self, now: Moment) {
        loop {
            let outputs = match self.receiver.as_mut() {
                Some(receiver) => receiver.take(),
                None => return,
            };
            if outputs.is_empty() {
                return;
            }
            for output in outputs {
                self.carry_out(output, now);
            }
        }
    }

    fn carry_out(&mut self, output: Output, now: Moment) {
        match output {
            Output::Open => {
                let opened = (self.open)(&self.device, self.baud_rate);
                let Some(receiver) = self.receiver.as_mut() else { return };
                match opened {
                    Ok(port) => {
                        self.port = Some(port);
                        receiver.opened(now.at);
                    }
                    Err(reason) => receiver.failed(reason, now.at),
                }
            }
            Output::Close => self.port = None,
            Output::Poll(poll) => {
                let frame = match poll {
                    Poll::Version => ubx::poll_version(),
                    Poll::Rf => ubx::poll_rf(),
                };
                let error = match self.port.as_mut() {
                    Some(port) => port.write(&frame).err().map(|e| e.to_string()),
                    None => Some(format!("{} is not open", self.device)),
                };
                if let Some(e) = error
                    && poll == Poll::Version
                {
                    eprintln!("[gnss] version poll failed: {e}");
                }
            }
            Output::Info(info) => self.on_info(*info, now),
            Output::Fix(fix) => self.publish_fix(fix),
        }
    }

    fn on_info(&mut self, mut info: GnssInfo, now: Moment) {
        info.timezone = self.timezone.clone();
        let _ = self.telemetry.send(json!({ "gnss": to_json(&info) }));
        if let Some(step) = self.clock.update(&info, now.unix_ms as i64) {
            self.jobs.push(Job::StepClock(step));
        }
    }

    fn publish_fix(&mut self, fix: GpsFix) {
        if let (Some(lat), Some(lng)) = (fix.lat, fix.lng) {
            let key = format!("{lat:.2},{lng:.2}");
            if key != self.zone_key {
                self.zone_key = key;
                if let Some(zone) = timezone::zone_for_position(lat, lng) {
                    timezone::note_gps_zone(zone);
                    // Again on every move, so a zone the phone set gives way.
                    self.jobs.push(Job::ApplyTimezone(zone.to_string()));
                    if self.timezone.as_deref() != Some(zone) {
                        self.timezone = Some(zone.to_string());
                        // Remembered, so the next start shows the right time before a fix.
                        self.jobs.push(Job::SaveTimezone(zone.to_string()));
                    }
                }
            }
        }
        let _ = self.telemetry.send(json!({ "gps": to_json(&fix) }));
    }
}

impl Drop for Gnss {
    fn drop(&mut self) {
        self.stop_receiver(Moment::now());
    }
}

async fn until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

async fn carry_out_job(core: &Core, job: Job) {
    match job {
        Job::ApplyTimezone(zone) => {
            tokio::spawn(timezone::apply(zone));
        }
        Job::SaveTimezone(zone) => {
            if let Err(e) = core.set_config(&json!({ "timezone": zone })).await {
                eprintln!("[gnss] timezone not saved: {e}");
            }
        }
        Job::StepClock(step) => {
            tokio::spawn(clock::step(step));
        }
    }
}

pub async fn run(core: Arc<Core>, telemetry: mpsc::UnboundedSender<Value>) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let mut state = core.hub.watch();
    let config = state.borrow_and_update().config.clone();
    let mut settings = Settings::of(&config);
    let mut gnss = Gnss::new(telemetry, Box::new(Port::open), Clock::new(clock::CLAIM_FILE));
    gnss.begin(&config, Moment::now());
    loop {
        for job in gnss.take_jobs() {
            carry_out_job(&core, job).await;
        }
        let wake = gnss.next_deadline();
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                let next = Settings::of(&state.borrow_and_update().config);
                if next != settings {
                    gnss.apply(&next, Moment::now());
                    settings = next;
                }
            }
            chunk = gnss.next_chunk() => gnss.read(chunk, Moment::now()),
            () = until(wake) => gnss.tick(Moment::now()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use livi_core_proto::config::defaults;

    use super::*;
    use crate::config_file::tests::TempDir;
    use crate::gnss::info::{FixMode, FixQuality};

    const GGA: &[u8] = b"$GPGGA,123519.00,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*69\r\n";
    const GSA: &[u8] = b"$GPGSA,A,3,04,05,,09,12,,,24,,,,,2.5,1.3,2.1*39\r\n";
    const RMC: &[u8] = b"$GPRMC,123519.00,A,4807.038,N,01131.000,E,022.4,084.4,230326,,,A*5B\r\n";

    struct Rig {
        gnss: Gnss,
        telemetry: mpsc::UnboundedReceiver<Value>,
        opened: Arc<Mutex<Vec<(String, u32)>>>,
        now: Moment,
        dir: TempDir,
    }

    impl Rig {
        fn new(config: &Config) -> Self {
            let dir = TempDir::new();
            let (tx, telemetry) = mpsc::unbounded_channel();
            let opened = Arc::new(Mutex::new(Vec::new()));
            let seen = opened.clone();
            let open: Opener = Box::new(move |device, baud_rate| {
                seen.lock().unwrap().push((device.to_string(), baud_rate));
                Ok(Port::fake(mpsc::unbounded_channel().1))
            });
            let gnss = Gnss::new(tx, open, Clock::new(dir.0.join("claim")));
            let now = Moment { at: Instant::now(), unix_ms: 1_774_269_319_000 };
            let mut rig = Self { gnss, telemetry, opened, now, dir };
            rig.gnss.begin(config, rig.now);
            rig
        }

        fn enabled() -> Self {
            Self::new(&Config { gps_enabled: true, ..defaults() })
        }

        fn opened(&self) -> Vec<(String, u32)> {
            self.opened.lock().unwrap().clone()
        }

        fn sent(&mut self) -> Vec<Value> {
            let mut all = Vec::new();
            while let Ok(v) = self.telemetry.try_recv() {
                all.push(v);
            }
            all
        }

        fn last_gnss(&mut self) -> Value {
            self.sent().into_iter().rev().find_map(|v| v.get("gnss").cloned()).unwrap()
        }

        fn feed(&mut self, bytes: &[u8]) {
            self.gnss.read(Some(Ok(bytes.to_vec())), self.now);
        }

        fn later(&mut self, by: Duration) {
            self.now.at += by;
            self.now.unix_ms += by.as_millis() as u64;
            self.gnss.tick(self.now);
        }

        fn fix(&mut self, lat: f64, lng: f64) {
            let fix = GpsFix { lat: Some(lat), lng: Some(lng), ..Default::default() };
            self.gnss.publish_fix(fix);
        }

        fn info(&mut self) {
            let info = GnssInfo { connected: true, satellites_used: 6.0, ..Default::default() };
            self.gnss.on_info(info, self.now);
        }

        fn apply(&mut self, enabled: bool, device: &str, baud_rate: u32) {
            let settings = Settings { enabled, device: device.into(), baud_rate };
            self.gnss.apply(&settings, self.now);
        }
    }

    #[test]
    fn a_receiver_runs_for_the_configured_device_only_while_enabled() {
        let rig = Rig::enabled();
        assert_eq!(rig.opened(), [(DEFAULT_DEVICE.to_string(), 38400)]);
        assert!(rig.gnss.receiver.is_some() && rig.gnss.port.is_some());

        let rig = Rig::new(&defaults());
        assert!(rig.opened().is_empty() && rig.gnss.receiver.is_none());

        let blank =
            Config { gps_enabled: true, gps_device: String::new(), gps_baud_rate: 0, ..defaults() };
        assert_eq!(Rig::new(&blank).opened(), [(DEFAULT_DEVICE.to_string(), 38400)]);
    }

    #[test]
    fn settings_changes_reopen_only_when_device_or_rate_changed() {
        let mut rig = Rig::enabled();
        rig.apply(true, DEFAULT_DEVICE, 38400);
        assert_eq!(rig.opened().len(), 1);
        rig.apply(true, "/dev/ttyUSB0", 38400);
        rig.apply(true, "/dev/ttyUSB0", 9600);
        let opened = rig.opened();
        assert_eq!(
            opened[1..],
            [("/dev/ttyUSB0".to_string(), 38400), ("/dev/ttyUSB0".to_string(), 9600)]
        );
    }

    #[test]
    fn turning_gps_off_stops_the_receiver_and_reports_it_once() {
        let mut rig = Rig::enabled();
        rig.feed(&[GGA, GSA, RMC].concat());
        rig.later(Duration::from_secs(1));
        assert!(rig.dir.0.join("claim").exists());
        rig.sent();
        rig.apply(false, "", 0);
        assert!(rig.gnss.receiver.is_none() && rig.gnss.port.is_none());
        assert!(!rig.dir.0.join("claim").exists());
        let sent = rig.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0]["gnss"],
            json!({
                "connected": false,
                "device": DEFAULT_DEVICE,
                "baudRate": 38400,
                "fixQuality": "none",
                "fixMode": "none",
                "satellitesUsed": 0,
                "satellitesVisible": 0,
                "satellites": [],
                "constellations": [],
                "timezone": "Europe/Berlin"
            })
        );
        rig.apply(false, "", 0);
        assert!(rig.sent().is_empty());
    }

    #[test]
    fn the_fix_and_the_receiver_state_go_to_the_telemetry() {
        let mut rig = Rig::enabled();
        assert_eq!(rig.last_gnss()["connected"], false);
        rig.feed(GGA);
        let sent = rig.sent();
        let gps = sent.iter().find_map(|v| v.get("gps")).unwrap();
        assert_eq!(gps["satellites"], 8);
        assert_eq!(gps["alt"], 545.4);
        let gnss = sent.iter().rev().find_map(|v| v.get("gnss")).unwrap();
        assert_eq!(gnss["connected"], true);

        rig.later(Duration::from_secs(1));
        let gnss = rig.last_gnss();
        assert_eq!(
            (&gnss["fixQuality"], &gnss["timezone"]),
            (&json!("gps"), &json!("Europe/Berlin"))
        );
    }

    #[test]
    fn a_trusted_clock_far_off_is_stepped() {
        let mut rig = Rig::enabled();
        rig.now.unix_ms -= 3_600_000;
        rig.feed(&[GGA, GSA, RMC].concat());
        rig.later(Duration::from_secs(1));
        let steps: Vec<Job> =
            rig.gnss.take_jobs().into_iter().filter(|j| matches!(j, Job::StepClock(_))).collect();
        assert_eq!(steps, [Job::StepClock(Step { epoch: 1_774_269_319, drift_s: 3599.0 })]);
        let info = rig.gnss.receiver.as_ref().unwrap().info();
        assert_eq!((info.fix_quality, info.fix_mode), (FixQuality::Gps, FixMode::ThreeD));
    }

    #[test]
    fn the_zone_follows_the_position() {
        let mut rig = Rig::enabled();
        rig.info();
        assert_eq!(rig.last_gnss().get("timezone"), None);

        rig.fix(53.3536, 10.5633);
        assert_eq!(
            rig.gnss.take_jobs(),
            [Job::ApplyTimezone("Europe/Berlin".into()), Job::SaveTimezone("Europe/Berlin".into())]
        );
        rig.info();
        assert_eq!(rig.last_gnss()["timezone"], "Europe/Berlin");

        rig.fix(53.3537, 10.5634);
        assert!(rig.gnss.take_jobs().is_empty());
        rig.fix(52.52, 13.405);
        assert_eq!(rig.gnss.take_jobs(), [Job::ApplyTimezone("Europe/Berlin".into())]);
        rig.fix(22.5726, 88.3639);
        assert_eq!(rig.gnss.take_jobs()[1], Job::SaveTimezone("Asia/Kolkata".into()));

        rig.gnss.publish_fix(GpsFix { alt: Some(12.0), ..Default::default() });
        rig.fix(999.0, 999.0);
        assert!(rig.gnss.take_jobs().is_empty());
        rig.info();
        assert_eq!(rig.last_gnss()["timezone"], "Asia/Kolkata");
    }

    #[test]
    fn the_remembered_zone_stands_until_gps_names_another() {
        let config = Config { timezone: "Asia/Kolkata".into(), ..defaults() };
        let mut rig = Rig::new(&config);
        assert_eq!(rig.gnss.take_jobs(), [Job::ApplyTimezone("Asia/Kolkata".into())]);
        rig.info();
        assert_eq!(rig.last_gnss()["timezone"], "Asia/Kolkata");
        rig.fix(22.5726, 88.3639);
        assert_eq!(rig.gnss.take_jobs(), [Job::ApplyTimezone("Asia/Kolkata".into())]);
        rig.fix(999.0, 999.0);
        rig.info();
        assert_eq!(rig.last_gnss()["timezone"], "Asia/Kolkata");
    }

    #[test]
    fn stopping_core_reports_the_receiver_gone_and_releases_the_clock() {
        let mut rig = Rig::enabled();
        rig.feed(&[GGA, GSA, RMC].concat());
        rig.later(Duration::from_secs(1));
        let claim = rig.dir.0.join("claim");
        assert!(claim.exists());
        let Rig { gnss, mut telemetry, dir, .. } = rig;
        drop(gnss);
        let mut last = None;
        while let Ok(v) = telemetry.try_recv() {
            last = v.get("gnss").cloned().or(last);
        }
        assert_eq!(last.unwrap()["connected"], false);
        assert!(!claim.exists());
        drop(dir);
    }

    #[test]
    fn a_port_that_cannot_be_opened_is_reported() {
        let dir = TempDir::new();
        let (tx, mut telemetry) = mpsc::unbounded_channel();
        let open: Opener = Box::new(|device, _| Err(format!("{device} not found")));
        let mut gnss = Gnss::new(tx, open, Clock::new(dir.0.join("claim")));
        gnss.begin(&Config { gps_enabled: true, ..defaults() }, Moment::now());
        let info = telemetry.try_recv().unwrap();
        assert_eq!(info["gnss"]["error"], "/dev/ttyAMA0 not found");
        assert!(gnss.next_deadline().is_some());
    }

    #[tokio::test]
    async fn chunks_come_from_the_open_port() {
        let dir = TempDir::new();
        let (tx, _telemetry) = mpsc::unbounded_channel();
        let (feed, chunks) = mpsc::unbounded_channel();
        let mut chunks = Some(chunks);
        let open: Opener = Box::new(move |_, _| Ok(Port::fake(chunks.take().unwrap())));
        let mut gnss = Gnss::new(tx, open, Clock::new(dir.0.join("claim")));
        gnss.begin(&Config { gps_enabled: true, ..defaults() }, Moment::now());
        feed.send(Ok(GGA.to_vec())).unwrap();
        assert_eq!(gnss.next_chunk().await, Some(Ok(GGA.to_vec())));
        drop(feed);
        let gone = gnss.next_chunk().await;
        gnss.read(gone, Moment::now());
        let info = gnss.receiver.as_ref().unwrap().info();
        assert_eq!(info.error.as_deref(), Some("/dev/ttyAMA0 closed"));
        assert!(gnss.port.is_none());
    }
}
