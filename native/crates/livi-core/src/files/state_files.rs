// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use livi_base::fs::{SHARED, write_atomic};
use livi_base::time::civil_from_days;
use livi_core_proto::state::{DabStation, FmStation, State};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::watch;

use crate::files::status_file::{self, Activity};
use crate::radio::Band;
use crate::server::Core;
use crate::vehicle::telemetry::CLIMATE;

pub const DIR: &str = "state";
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
const STATUS: Kind = Kind {
    name: "status.json",
    version: 2,
    pace: QUICK,
};
const CLIMATE_FILE: Kind = Kind {
    name: "climate.json",
    version: 1,
    pace: QUICK,
};
const GPS: Kind = Kind {
    name: "gps.json",
    version: 3,
    pace: Duration::from_secs(1),
};
const MEDIA: Kind = Kind {
    name: "media.json",
    version: 1,
    pace: QUICK,
};
const NAVIGATION: Kind = Kind {
    name: "navigation.json",
    version: 1,
    pace: QUICK,
};
const VEHICLE: Kind = Kind {
    name: "vehicle.json",
    version: 1,
    pace: Duration::from_secs(1),
};
const SDR: Kind = Kind {
    name: "sdr.json",
    version: 1,
    pace: QUICK,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sdr {
    pub stations: Vec<DabStation>,
    pub last_station: Option<u32>,
    pub fm_stations: Vec<FmStation>,
    pub last_fm: Option<u32>,
}

pub fn load_sdr(user_data: &Path) -> Sdr {
    payload_of(user_data, &SDR)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn save_sdr(user_data: &Path, sdr: &Sdr) {
    write(
        &user_data.join(DIR).join(SDR.name),
        &render(SDR.version, sdr, SystemTime::now()),
    );
}

pub const KEPT_WHILE_PARKED: [&str; 10] = [
    "fuelPct",
    "rangeKm",
    "rangeFuelKm",
    "rangeElectricKm",
    "consumptionAvgLPer100Km",
    "odometerKm",
    "odometerTripKm",
    "batteryLevelKwh",
    "batteryCapacityKwh",
    "tirePressure",
];

fn kept_value(v: &Value) -> Option<Value> {
    match v {
        Value::Number(_) => Some(v.clone()),
        Value::Object(fields) => {
            let numbers: Map<String, Value> = fields
                .iter()
                .filter(|(_, n)| n.is_number())
                .map(|(k, n)| (k.clone(), n.clone()))
                .collect();
            (!numbers.is_empty()).then_some(Value::Object(numbers))
        }
        _ => None,
    }
}

fn kept_of(fields: &Map<String, Value>) -> Map<String, Value> {
    KEPT_WHILE_PARKED
        .iter()
        .filter_map(|k| Some(((*k).to_string(), kept_value(fields.get(*k)?)?)))
        .collect()
}

#[derive(Serialize)]
struct File<'a, T: Serialize> {
    version: u32,
    timestamp: &'a str,
    payload: &'a T,
}

fn iso(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

fn render<T: Serialize>(version: u32, payload: &T, now: SystemTime) -> String {
    let file = File {
        version,
        timestamp: &iso(now),
        payload,
    };
    let mut text = serde_json::to_string_pretty(&file).unwrap_or_default();
    text.push('\n');
    text
}

fn write(target: &Path, text: &str) {
    if let Err(e) = write_atomic(target, text, SHARED) {
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

    async fn offer<T: Serialize>(&mut self, payload: Option<T>) {
        let Some(payload) = payload else { return };
        let value = serde_json::to_value(&payload).unwrap_or(Value::Null);
        if self.seen.as_ref() == Some(&value) {
            return;
        }
        write(
            &self.path,
            &render(self.version, &payload, SystemTime::now()),
        );
        self.seen = Some(value);
        tokio::time::sleep(self.pace).await;
    }
}

fn climate_of(state: &State) -> Option<Value> {
    state.telemetry.get(CLIMATE).cloned()
}

fn gps_of(state: &State) -> Option<Value> {
    let fix = state.telemetry.get("gps").cloned();
    let mut receiver = state.telemetry.get("gnss").cloned();
    if let Some(Value::Object(fields)) = &mut receiver {
        fields.remove("satellites");
    }
    (fix.is_some() || receiver.is_some()).then(|| json!({ "fix": fix, "receiver": receiver }))
}

fn vehicle_of(state: &State) -> Option<Value> {
    let kept = kept_of(&state.telemetry);
    (!kept.is_empty()).then_some(Value::Object(kept))
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
        let (active, radio) = {
            let s = state.borrow_and_update();
            let radio = if s.dab.active {
                Some(Band::Dab)
            } else if s.fm.active {
                Some(Band::Fm)
            } else {
                None
            };
            (s.sessions.active, radio)
        };
        let payload = status_file::payload(
            &path.borrow_and_update(),
            active,
            *activity.borrow_and_update(),
            radio,
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
    tokio::spawn(status(
        core.clone(),
        activity,
        Writer::new(user_data, &STATUS),
    ));
    let files: [(&Kind, Payload); 5] = [
        (&CLIMATE_FILE, climate_of),
        (&GPS, gps_of),
        (&VEHICLE, vehicle_of),
        (&MEDIA, media_of),
        (&NAVIGATION, navigation_of),
    ];
    for (kind, of) in files {
        tokio::spawn(follow(core.hub.watch(), Writer::new(user_data, kind), of));
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Saved {
    pub climate: Option<Map<String, Value>>,
    pub gps: Option<Map<String, Value>>,
    pub vehicle: Option<Map<String, Value>>,
    pub ui_path: Option<String>,
    pub radio: Option<Band>,
}

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
    let vehicle = object(payload_of(user_data, &VEHICLE))
        .map(|v| kept_of(&v))
        .filter(|v| !v.is_empty());
    let status = payload_of(user_data, &STATUS).unwrap_or(Value::Null);
    Saved {
        climate: object(payload_of(user_data, &CLIMATE_FILE)),
        gps: object(gps),
        vehicle,
        ui_path: status["ui"]["path"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(str::to_string),
        radio: status_file::band(status["media"]["source"].as_str().unwrap_or("")),
    }
}

#[cfg(test)]
mod tests {
    use livi_core_proto::state::{Front, PerScreen};

    use super::*;
    use crate::files::config_file::defaults;
    use crate::files::config_file::tests::TempDir;

    fn state(telemetry: Value) -> State {
        State {
            front: PerScreen {
                main: Front::Livi,
                cluster: Front::Livi,
                aux: Front::Livi,
            },
            sessions: Default::default(),
            now_playing: Default::default(),
            video_playback: None,
            telemetry: telemetry.as_object().cloned().unwrap_or_default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            cluster: Default::default(),
            dab: Default::default(),
            fm: Default::default(),
            cameras: Vec::new(),
            mfi: Default::default(),
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
        assert_eq!(
            iso(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00.000Z"
        );
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
        assert_eq!(
            (&first["version"], &first["payload"]),
            (&json!(1), &json!({ "auto": true }))
        );
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
        assert_eq!(
            climate_of(&full),
            Some(json!({ "auto": true, "source": "carplay" }))
        );
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
        write(
            &dir.0.join(DIR).join("climate.json"),
            &render(1, &json!({ "auto": false }), at),
        );
        let gps = json!({ "fix": { "lat": 48.1, "lng": 11.5 }, "receiver": null });
        write(&dir.0.join(DIR).join("gps.json"), &render(3, &gps, at));
        let back = saved(&dir.0);
        assert_eq!(
            back.climate.map(Value::Object),
            Some(json!({ "auto": false }))
        );
        assert_eq!(
            back.gps.map(Value::Object),
            Some(json!({ "lat": 48.1, "lng": 11.5 }))
        );

        write(&dir.0.join(DIR).join("gps.json"), &render(2, &gps, at));
        std::fs::write(dir.0.join(DIR).join("climate.json"), "{ broken").unwrap();
        assert_eq!(
            saved(&dir.0),
            Saved::default(),
            "another version or broken JSON counts as none"
        );
        write(
            &dir.0.join(DIR).join("climate.json"),
            &render(1, &json!([1]), at),
        );
        assert_eq!(saved(&dir.0).climate, None);
    }

    #[test]
    fn the_next_start_reads_the_last_page_and_source_back() {
        let dir = TempDir::new();
        let payload =
            json!({ "ui": { "path": "/media/dabplus" }, "media": { "source": "dabplus" } });
        write(
            &dir.0.join(DIR).join("status.json"),
            &render(STATUS.version, &payload, UNIX_EPOCH),
        );
        let back = saved(&dir.0);
        assert_eq!(back.ui_path.as_deref(), Some("/media/dabplus"));
        assert_eq!(back.radio, Some(Band::Dab));
        let old = json!({ "ui": { "path": "/media" } });
        write(
            &dir.0.join(DIR).join("status.json"),
            &render(1, &old, UNIX_EPOCH),
        );
        assert_eq!(saved(&dir.0), Saved::default(), "the old shape is ignored");
    }

    #[test]
    fn the_vehicle_file_keeps_what_a_parked_car_keeps() {
        assert_eq!(vehicle_of(&state(json!({ "speedKph": 50 }))), None);
        let driving = state(json!({
            "fuelPct": 42,
            "rangeKm": 380.5,
            "odometerKm": 123456,
            "speedKph": 50,
            "consumptionLPer100Km": 7.6,
            "batteryCapacityKwh": "77"
        }));
        assert_eq!(
            vehicle_of(&driving),
            Some(json!({ "fuelPct": 42, "rangeKm": 380.5, "odometerKm": 123456 }))
        );
    }

    #[test]
    fn the_next_start_reads_the_vehicle_back() {
        let dir = TempDir::new();
        let path = dir.0.join(DIR).join("vehicle.json");
        let left = json!({ "fuelPct": 42, "odometerKm": 123456, "rpm": 900, "rangeKm": null });
        write(&path, &render(1, &left, UNIX_EPOCH));
        assert_eq!(
            saved(&dir.0).vehicle.map(Value::Object),
            Some(json!({ "fuelPct": 42, "odometerKm": 123456 }))
        );

        write(&path, &render(1, &json!({ "rpm": 900 }), UNIX_EPOCH));
        assert_eq!(saved(&dir.0).vehicle, None, "nothing kept counts as none");
        write(&path, &render(2, &left, UNIX_EPOCH));
        assert_eq!(saved(&dir.0).vehicle, None);
    }

    #[test]
    fn tire_pressures_are_kept_as_a_block_of_numbers() {
        let parked = state(json!({
            "tirePressure": { "frontLeftKpa": 240, "rearLeftKpa": "low" },
            "climate": { "auto": true }
        }));
        assert_eq!(
            vehicle_of(&parked),
            Some(json!({ "tirePressure": { "frontLeftKpa": 240 } }))
        );
        assert_eq!(
            vehicle_of(&state(json!({ "tirePressure": { "rearLeftKpa": "low" } }))),
            None
        );
        assert_eq!(vehicle_of(&state(json!({ "tirePressure": [240] }))), None);
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

    #[test]
    fn the_station_list_lives_in_sdr_json() {
        let dir = TempDir::new();
        assert_eq!(load_sdr(&dir.0), Sdr::default());
        let sdr = Sdr {
            stations: vec![DabStation {
                service_id: 0xd210,
                label: "Dlf".into(),
                short_label: "Dlf".into(),
                ensemble: "DR Deutschland".into(),
                channel: "5C".into(),
                frequency_hz: 178_352_000,
                favorite: false,
                hidden: false,
                logo: None,
            }],
            last_station: Some(0xd210),
            fm_stations: vec![FmStation {
                khz: 98_100,
                name: "NDR 2".into(),
                pi: Some(0xd3c2),
                favorite: true,
                hidden: false,
            }],
            last_fm: Some(98_100),
        };
        save_sdr(&dir.0, &sdr);
        let file = read(&dir, "sdr.json");
        assert_eq!(file["version"], 1);
        assert_eq!(file["payload"]["lastStation"], 0xd210);
        assert_eq!(file["payload"]["stations"][0]["channel"], "5C");
        assert_eq!(file["payload"]["fmStations"][0]["khz"], 98_100);
        assert_eq!(load_sdr(&dir.0), sdr);

        std::fs::write(dir.0.join(DIR).join("sdr.json"), "{ broken").unwrap();
        assert_eq!(load_sdr(&dir.0), Sdr::default());
    }
}
