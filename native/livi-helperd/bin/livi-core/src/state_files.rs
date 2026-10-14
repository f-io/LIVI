//! LIVI's state as files in state/, read by programs outside LIVI. Climate and the last
//! position come back from there at the next start.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use livi_core_proto::state::State;
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::sync::watch;

use crate::server::Core;
use crate::status_file::{self, Activity};
use crate::telemetry::CLIMATE;

pub const DIR: &str = "state";
/// Where these files lay before, read on in silence they would only mislead.
const MOVED: [&str; 5] = [
    "statusData.json",
    "climateData.json",
    "gpsData.json",
    "mediaData.json",
    "navigationData.json",
];

struct Kind {
    name: &'static str,
    version: u32,
    pace: Duration,
}

const QUICK: Duration = Duration::from_millis(50);
const STATUS: Kind = Kind { name: "status.json", version: 1, pace: QUICK };
const CLIMATE_FILE: Kind = Kind { name: "climate.json", version: 1, pace: QUICK };
/// A moving car changes it every second.
const GPS: Kind = Kind { name: "gps.json", version: 3, pace: Duration::from_secs(1) };
const MEDIA: Kind = Kind { name: "media.json", version: 1, pace: QUICK };
const NAVIGATION: Kind = Kind { name: "navigation.json", version: 1, pace: QUICK };

#[derive(Serialize)]
struct File<'a, T: Serialize> {
    version: u32,
    timestamp: &'a str,
    payload: &'a T,
}

/// UTC with milliseconds, 2026-10-05T18:00:00.000Z.
fn iso(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days to a civil date, after Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

fn render<T: Serialize>(version: u32, payload: &T, now: SystemTime) -> String {
    let file = File { version, timestamp: &iso(now), payload };
    let mut text = serde_json::to_string_pretty(&file).unwrap_or_default();
    text.push('\n');
    text
}

/// A reader gets the old file or the new one, never half of it.
fn write(target: &Path, text: &str) {
    let tmp = target.with_extension("json.tmp");
    let written = target
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&tmp, text))
        .and_then(|()| std::fs::rename(&tmp, target));
    if let Err(e) = written {
        eprintln!("[core] {} not written: {e}", target.display());
    }
}

struct Writer {
    path: PathBuf,
    version: u32,
    pace: Duration,
    seen: Option<Value>,
}

impl Writer {
    fn new(user_data: &Path, kind: &Kind) -> Self {
        Self {
            path: user_data.join(DIR).join(kind.name),
            version: kind.version,
            pace: kind.pace,
            seen: None,
        }
    }

    /// Writes when the payload changed, then rests for its pace.
    async fn offer<T: Serialize>(&mut self, payload: Option<T>) {
        let Some(payload) = payload else { return };
        let value = serde_json::to_value(&payload).unwrap_or(Value::Null);
        if self.seen.as_ref() == Some(&value) {
            return;
        }
        write(&self.path, &render(self.version, &payload, SystemTime::now()));
        self.seen = Some(value);
        tokio::time::sleep(self.pace).await;
    }
}

fn climate_of(state: &State) -> Option<Value> {
    state.telemetry.get(CLIMATE).cloned()
}

/// The satellites stay out, they change with every sentence.
fn gps_of(state: &State) -> Option<Value> {
    let fix = state.telemetry.get("gps").cloned();
    let mut receiver = state.telemetry.get("gnss").cloned();
    if let Some(Value::Object(fields)) = &mut receiver {
        fields.remove("satellites");
    }
    (fix.is_some() || receiver.is_some()).then(|| json!({ "fix": fix, "receiver": receiver }))
}

fn media_of(state: &State) -> Option<Value> {
    serde_json::to_value(&state.now_playing).ok()
}

fn navigation_of(state: &State) -> Option<Value> {
    serde_json::to_value(&state.navigation).ok()
}

type Payload = fn(&State) -> Option<Value>;

async fn follow(mut state: watch::Receiver<State>, mut writer: Writer, of: Payload) {
    loop {
        let payload = of(&state.borrow_and_update());
        writer.offer(payload).await;
        if state.changed().await.is_err() {
            return;
        }
    }
}

async fn status(core: Arc<Core>, mut activity: watch::Receiver<Activity>, mut writer: Writer) {
    let mut state = core.hub.watch();
    let mut path = core.ui_path.subscribe();
    loop {
        let payload = status_file::payload(
            &path.borrow_and_update(),
            state.borrow_and_update().sessions.active,
            *activity.borrow_and_update(),
        );
        writer.offer(Some(payload)).await;
        tokio::select! {
            changed = state.changed() => if changed.is_err() { return },
            changed = activity.changed() => if changed.is_err() { return },
            changed = path.changed() => if changed.is_err() { return },
        }
    }
}

fn tidy(user_data: &Path) {
    for name in MOVED {
        let old = user_data.join(name);
        if !old.exists() {
            continue;
        }
        match std::fs::remove_file(&old) {
            Ok(()) => println!("[core] removed {name}, LIVI writes its state into {DIR}/ now"),
            Err(e) => eprintln!("[core] cannot remove {}: {e}", old.display()),
        }
    }
}

pub fn start(core: &Arc<Core>, activity: watch::Receiver<Activity>, user_data: &Path) {
    tidy(user_data);
    tokio::spawn(status(core.clone(), activity, Writer::new(user_data, &STATUS)));
    let files: [(&Kind, Payload); 4] = [
        (&CLIMATE_FILE, climate_of),
        (&GPS, gps_of),
        (&MEDIA, media_of),
        (&NAVIGATION, navigation_of),
    ];
    for (kind, of) in files {
        tokio::spawn(follow(core.hub.watch(), Writer::new(user_data, kind), of));
    }
}

/// What the last run left behind for the next start.
#[derive(Debug, Default, PartialEq)]
pub struct Saved {
    pub climate: Option<Map<String, Value>>,
    /// The last fix.
    pub gps: Option<Map<String, Value>>,
}

/// None when the file is missing, broken or of another version.
fn payload_of(user_data: &Path, kind: &Kind) -> Option<Value> {
    let text = std::fs::read_to_string(user_data.join(DIR).join(kind.name)).ok()?;
    let mut file: Value = serde_json::from_str(&text).ok()?;
    if file.get("version")?.as_u64()? != u64::from(kind.version) {
        return None;
    }
    Some(file.get_mut("payload")?.take())
}

pub fn saved(user_data: &Path) -> Saved {
    let object = |v: Option<Value>| match v? {
        Value::Object(fields) => Some(fields),
        _ => None,
    };
    let gps = payload_of(user_data, &GPS).and_then(|mut p| p.get_mut("fix").map(Value::take));
    Saved { climate: object(payload_of(user_data, &CLIMATE_FILE)), gps: object(gps) }
}

#[cfg(test)]
mod tests {
    use livi_core_proto::state::{Front, PerScreen};

    use super::*;
    use crate::config_file::defaults;
    use crate::config_file::tests::TempDir;

    fn state(telemetry: Value) -> State {
        State {
            front: PerScreen { main: Front::Livi, cluster: Front::Livi, aux: Front::Livi },
            sessions: Default::default(),
            now_playing: Default::default(),
            telemetry: telemetry.as_object().cloned().unwrap_or_default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            config: defaults(),
        }
    }

    fn read(dir: &TempDir, name: &str) -> Value {
        let text = std::fs::read_to_string(dir.0.join(DIR).join(name)).unwrap();
        assert!(text.ends_with("}\n"));
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn dates_are_written_in_utc() {
        assert_eq!(iso(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00.000Z");
        let at = UNIX_EPOCH + Duration::from_millis(1_759_687_200_123);
        assert_eq!(
            serde_json::from_str::<Value>(&render(3, &json!({ "a": 1 }), at)).unwrap(),
            json!({ "version": 3, "timestamp": "2025-10-05T18:00:00.123Z", "payload": { "a": 1 } })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_file_is_written_when_its_payload_changes() {
        let dir = TempDir::new();
        let mut writer = Writer::new(&dir.0, &CLIMATE_FILE);
        writer.offer(None::<Value>).await;
        assert!(!dir.0.join(DIR).exists());
        writer.offer(Some(json!({ "auto": true }))).await;
        let first = read(&dir, "climate.json");
        assert_eq!((&first["version"], &first["payload"]), (&json!(1), &json!({ "auto": true })));
        std::fs::remove_file(dir.0.join(DIR).join("climate.json")).unwrap();
        writer.offer(Some(json!({ "auto": true }))).await;
        assert!(
            !dir.0.join(DIR).join("climate.json").exists(),
            "the same payload is not rewritten"
        );
        writer.offer(Some(json!({ "auto": false }))).await;
        assert_eq!(read(&dir, "climate.json")["payload"]["auto"], false);
    }

    #[test]
    fn each_file_takes_its_part_of_the_state() {
        let empty = state(json!({}));
        assert_eq!(climate_of(&empty), None);
        assert_eq!(gps_of(&empty), None);
        assert_eq!(media_of(&empty).unwrap()["title"], Value::Null);
        assert_eq!(navigation_of(&empty).unwrap()["active"], Value::Null);

        let full = state(json!({
            "climate": { "auto": true, "source": "carplay" },
            "gps": { "lat": 48.1, "lng": 11.5 },
            "gnss": { "connected": true, "satellites": [{ "id": 4 }] }
        }));
        assert_eq!(climate_of(&full), Some(json!({ "auto": true, "source": "carplay" })));
        assert_eq!(
            gps_of(&full),
            Some(json!({ "fix": { "lat": 48.1, "lng": 11.5 }, "receiver": { "connected": true } }))
        );
        let receiver_only = state(json!({ "gnss": { "connected": false } }));
        assert_eq!(gps_of(&receiver_only).unwrap()["fix"], Value::Null);
    }

    #[test]
    fn the_next_start_reads_climate_and_the_last_fix_back() {
        let dir = TempDir::new();
        assert_eq!(saved(&dir.0), Saved::default());
        let at = UNIX_EPOCH;
        write(&dir.0.join(DIR).join("climate.json"), &render(1, &json!({ "auto": false }), at));
        let gps = json!({ "fix": { "lat": 48.1, "lng": 11.5 }, "receiver": null });
        write(&dir.0.join(DIR).join("gps.json"), &render(3, &gps, at));
        let back = saved(&dir.0);
        assert_eq!(back.climate.map(Value::Object), Some(json!({ "auto": false })));
        assert_eq!(back.gps.map(Value::Object), Some(json!({ "lat": 48.1, "lng": 11.5 })));

        write(&dir.0.join(DIR).join("gps.json"), &render(2, &gps, at));
        std::fs::write(dir.0.join(DIR).join("climate.json"), "{ broken").unwrap();
        assert_eq!(
            saved(&dir.0),
            Saved::default(),
            "another version or broken JSON counts as none"
        );
        write(&dir.0.join(DIR).join("climate.json"), &render(1, &json!([1]), at));
        assert_eq!(saved(&dir.0).climate, None);
    }

    #[test]
    fn the_files_from_before_go() {
        let dir = TempDir::new();
        for name in ["statusData.json", "mediaData.json", "config.json"] {
            std::fs::write(dir.0.join(name), "{}").unwrap();
        }
        tidy(&dir.0);
        assert!(!dir.0.join("statusData.json").exists());
        assert!(!dir.0.join("mediaData.json").exists());
        assert!(dir.0.join("config.json").exists());
    }
}
