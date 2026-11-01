// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_cp::helper_sock::HelperSock;
use livi_cp::manager::{CpCmd, CpHandle};
use livi_cp::vehicle_state::{GAUGE_KEYS, Notice};
use serde_json::{Map, Value};

use crate::aa_telemetry::{LOW_FUEL_PCT, gear};
use crate::nmea::{self, Position};
use crate::telemetry::{CLIMATE, NOTIFICATION, SOURCE};

pub const FROM_CARPLAY: &str = "carplay";

const NEUTRAL: i64 = 0;
const PARK: i64 = 101;

pub struct CpTelemetry {
    helper: HelperSock,
    night: Option<bool>,
    cluster_night: Option<bool>,
    limited_ui: Option<bool>,
    video_allowed: Option<bool>,
}

fn changed(prev: &Map<String, Value>, next: &Map<String, Value>, key: &str) -> bool {
    next.contains_key(key) && prev.get(key) != next.get(key)
}

fn num(map: &Map<String, Value>, key: &str) -> Option<f64> {
    map.get(key).and_then(Value::as_f64)
}

const VEHICLE_STATUS_KEYS: [&str; 10] = [
    "rangeKm",
    "ambientC",
    "fuelPct",
    "rangeFuelKm",
    "rangeElectricKm",
    "batteryCapacityKwh",
    "batteryLevelKwh",
    "charging",
    "activeConnector",
    "rangeMaxElectricKm",
];

fn km(range: f64) -> Value {
    (range.round().clamp(0.0, 65535.0) as u32).into()
}

fn wh(kwh: f64) -> Value {
    ((kwh * 1000.0).round().clamp(0.0, f64::from(u32::MAX)) as u32).into()
}

fn connector(name: &str) -> Option<u8> {
    match name {
        "Combo1" => Some(0),
        "Combo2" => Some(1),
        "J1772" => Some(2),
        "Chademo" => Some(3),
        "Mennekes" => Some(4),
        "Gbt" => Some(5),
        "TeslaSupercharger" => Some(7),
        _ => None,
    }
}

impl CpTelemetry {
    pub fn new(helper: HelperSock) -> Self {
        Self {
            helper,
            night: None,
            cluster_night: None,
            limited_ui: None,
            video_allowed: None,
        }
    }

    pub fn follow(&mut self, prev: &Map<String, Value>, next: &Map<String, Value>, cp: &CpHandle) {
        if changed(prev, next, "nightMode")
            && let Some(night) = next.get("nightMode").and_then(Value::as_bool)
            && self.night != Some(night)
        {
            self.night = Some(night);
            cp.send(CpCmd::NightMode(night));
        }
        if (changed(prev, next, "nightMode") || changed(prev, next, "clusterNightMode"))
            && let Some(night) = next
                .get("clusterNightMode")
                .and_then(Value::as_bool)
                .or_else(|| next.get("nightMode").and_then(Value::as_bool))
            && self.cluster_night != Some(night)
        {
            self.cluster_night = Some(night);
            cp.send(CpCmd::ClusterNightMode(night));
        }
        if (changed(prev, next, "gear") || changed(prev, next, "reverse"))
            && let Some(g) = gear(
                next.get("gear"),
                next.get("reverse").and_then(Value::as_bool),
            )
        {
            let limited = !matches!(g, NEUTRAL | PARK);
            if self.limited_ui != Some(limited) {
                self.limited_ui = Some(limited);
                cp.send(CpCmd::LimitedUi(limited));
            }
            let parked = g == PARK;
            if self.video_allowed != Some(parked) {
                self.video_allowed = Some(parked);
                cp.send(CpCmd::VideoAllowed(parked));
            }
        }
        if VEHICLE_STATUS_KEYS.iter().any(|k| changed(prev, next, k)) {
            let mut status = Map::new();
            let mut put = |key: &str, value: Option<Value>| {
                if let Some(value) = value {
                    status.insert(key.into(), value);
                }
            };
            put("range", num(next, "rangeKm").map(km));
            put("rangeFuel", num(next, "rangeFuelKm").map(km));
            put("rangeElectric", num(next, "rangeElectricKm").map(km));
            put(
                "outsideTemperature",
                num(next, "ambientC").map(|c| (c.round() as i64).into()),
            );
            put(
                "rangeWarning",
                num(next, "fuelPct").map(|pct| (pct < LOW_FUEL_PCT).into()),
            );
            let capacity = num(next, "batteryCapacityKwh").filter(|c| *c > 0.0);
            let level = num(next, "batteryLevelKwh")
                .or_else(|| Some(num(next, "fuelPct")? / 100.0 * capacity?));
            put("batteryMinWh", capacity.map(|_| 0.into()));
            put("batteryMaxWh", capacity.map(wh));
            put("batteryWh", level.map(wh));
            let thousandths =
                |pct: f64| Value::from((pct.clamp(0.0, 100.0) * 1000.0).round() as u32);
            put("batteryPercentE3", num(next, "fuelPct").map(thousandths));
            put(
                "charging",
                next.get("charging").filter(|v| v.is_boolean()).cloned(),
            );
            put(
                "activeConnector",
                next.get("activeConnector")
                    .and_then(Value::as_str)
                    .and_then(connector)
                    .map(Value::from),
            );
            put("rangeMaxElectric", num(next, "rangeMaxElectricKm").map(km));
            if !status.is_empty() {
                let helper = self.helper.clone();
                let status = Value::Object(status);
                tokio::spawn(async move { helper.send_vehicle_status(&status).await });
            }
        }
        let gauges: Map<String, Value> = GAUGE_KEYS
            .iter()
            .filter(|k| changed(prev, next, k))
            .filter_map(|k| Some(((*k).to_string(), next.get(*k)?.clone())))
            .collect();
        if !gauges.is_empty() {
            cp.send(CpCmd::Gauges(gauges));
        }
        if changed(prev, next, "gps")
            && let Some(gps) = next.get("gps").and_then(Value::as_object)
            && let (Some(lat), Some(lng)) = (num(gps, "lat"), num(gps, "lng"))
        {
            let nmea = nmea::encode(&Position {
                lat,
                lng,
                alt: num(gps, "alt"),
                heading: num(gps, "heading"),
                speed_ms: num(gps, "speedMs"),
                fix_ms: num(gps, "fixTs"),
                accuracy_m: num(gps, "accuracyM"),
            });
            let helper = self.helper.clone();
            tokio::spawn(async move { helper.send_location(&nmea).await });
        }
        if changed(prev, next, NOTIFICATION) {
            let note = next.get(NOTIFICATION).and_then(Value::as_object);
            let text = |key: &str| {
                note.and_then(|n| n.get(key))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let open = note.and_then(|n| n.get("open")).and_then(Value::as_bool) == Some(true);
            let notice = open.then(|| Notice {
                title: text("title"),
                text: text("text"),
                symbol: text("symbol"),
            });
            cp.send(CpCmd::Notification(notice));
        }
        if changed(prev, next, CLIMATE)
            && let Some(climate) = next.get(CLIMATE).and_then(Value::as_object)
            && climate.get(SOURCE).and_then(Value::as_str) != Some(FROM_CARPLAY)
        {
            let before = prev.get(CLIMATE).and_then(Value::as_object);
            let changes: Map<String, Value> = climate
                .iter()
                .filter(|(key, v)| *key != SOURCE && before.and_then(|b| b.get(*key)) != Some(*v))
                .map(|(key, v)| (key.clone(), v.clone()))
                .collect();
            if !changes.is_empty() {
                cp.send(CpCmd::Climate(changes));
            }
        }
    }

    pub fn hydrate(&mut self, snap: &Map<String, Value>, cp: &CpHandle) {
        self.night = None;
        self.cluster_night = None;
        self.limited_ui = None;
        self.video_allowed = None;
        let mut rest = snap.clone();
        let climate = rest.remove(CLIMATE);
        self.follow(&Map::new(), &rest, cp);
        if let Some(Value::Object(mut climate)) = climate {
            climate.remove(SOURCE);
            cp.send(CpCmd::Climate(climate));
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;
    use tokio::sync::mpsc;

    use super::*;
    use crate::config_file::tests::TempDir;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    fn helper(dir: &TempDir) -> (HelperSock, mpsc::UnboundedReceiver<String>) {
        let path = dir.0.join("cp-bt.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (rd, mut wr) = stream.into_split();
                let mut line = String::new();
                let _ = BufReader::new(rd).read_line(&mut line).await;
                let _ = tx.send(line.trim_end().to_string());
                let _ = wr.write_all(b"{\"ok\":true}\n").await;
            }
        });
        (HelperSock::new(path), rx)
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn changes_reach_the_phone_and_a_new_session_hears_everything() {
        let dir = TempDir::new();
        let (sock, mut lines) = helper(&dir);
        let (tx, mut cmds) = mpsc::unbounded_channel();
        let cp = CpHandle::from_sender(tx);
        let mut t = CpTelemetry::new(sock);
        let first = obj(json!({ "nightMode": true, "speedKph": 50, "ts": 1 }));
        t.follow(&Map::new(), &first, &cp);
        assert_eq!(cmds.try_recv(), Ok(CpCmd::NightMode(true)));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::ClusterNightMode(true)));

        let second = obj(json!({
            "nightMode": true,
            "rangeKm": 70000.4,
            "ambientC": -3.6,
            "fuelPct": 9.5,
            "ts": 2
        }));
        t.follow(&first, &second, &cp);
        assert_eq!(
            cmds.try_recv(),
            Ok(CpCmd::Gauges(obj(
                json!({ "fuelPct": 9.5, "rangeKm": 70000.4 })
            )))
        );
        assert!(cmds.try_recv().is_err());
        assert_eq!(
            next(&mut lines).await,
            r#"vehicle-status {"batteryPercentE3":9500,"outsideTemperature":-4,"range":65535,"rangeWarning":true}"#
        );

        let third = obj(
            json!({ "gps": { "lat": 1.0, "lng": 1.0, "fixTs": 1_759_665_845_000u64 }, "ts": 3 }),
        );
        t.follow(&second, &third, &cp);
        let location = next(&mut lines).await;
        assert!(location.starts_with("location "));

        t.follow(&second, &obj(json!({ "fuelPct": 10 })), &cp);
        assert_eq!(
            next(&mut lines).await,
            r#"vehicle-status {"batteryPercentE3":10000,"rangeWarning":false}"#
        );
        assert_eq!(
            cmds.try_recv(),
            Ok(CpCmd::Gauges(obj(json!({ "fuelPct": 10 }))))
        );

        let hybrid = obj(json!({ "rangeFuelKm": 512.6, "rangeElectricKm": 41.2 }));
        t.follow(&second, &hybrid, &cp);
        assert_eq!(
            next(&mut lines).await,
            r#"vehicle-status {"rangeElectric":41,"rangeFuel":513}"#
        );
        assert!(cmds.try_recv().is_err());

        let ev = obj(json!({
            "batteryCapacityKwh": 77.0,
            "fuelPct": 50,
            "charging": true,
            "activeConnector": "Combo2",
            "rangeMaxElectricKm": 520.4
        }));
        t.follow(&second, &ev, &cp);
        assert_eq!(
            next(&mut lines).await,
            r#"vehicle-status {"activeConnector":1,"batteryMaxWh":77000,"batteryMinWh":0,"batteryPercentE3":50000,"batteryWh":38500,"charging":true,"rangeMaxElectric":520,"rangeWarning":false}"#,
            "the level comes from fuelPct and the capacity when the car does not tell it"
        );
        let _ = cmds.try_recv();
        let odd = obj(json!({ "rangeKm": 10, "activeConnector": "Other", "charging": "yes" }));
        t.follow(&ev, &odd, &cp);
        assert_eq!(
            next(&mut lines).await,
            r#"vehicle-status {"range":10}"#,
            "no plug number, no word"
        );
        let _ = cmds.try_recv();

        t.hydrate(&second, &cp);
        assert_eq!(cmds.try_recv(), Ok(CpCmd::NightMode(true)));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::ClusterNightMode(true)));
        assert!(matches!(cmds.try_recv(), Ok(CpCmd::Gauges(_))));
        assert!(next(&mut lines).await.starts_with("vehicle-status "));

        t.follow(&second, &obj(json!({ "gps": { "lat": 1.0 } })), &cp);
        t.follow(&second, &obj(json!({ "nightMode": "dusk" })), &cp);
        assert!(cmds.try_recv().is_err());
    }

    #[test]
    fn the_cluster_follows_the_night_mode_until_it_has_its_own() {
        let (tx, mut cmds) = mpsc::unbounded_channel();
        let cp = CpHandle::from_sender(tx);
        let mut t = CpTelemetry::new(HelperSock::default());
        let day = obj(json!({ "nightMode": false }));
        t.follow(&Map::new(), &day, &cp);
        assert_eq!(cmds.try_recv(), Ok(CpCmd::NightMode(false)));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::ClusterNightMode(false)));
        let dark_cluster = obj(json!({ "nightMode": false, "clusterNightMode": true }));
        t.follow(&day, &dark_cluster, &cp);
        assert_eq!(cmds.try_recv(), Ok(CpCmd::ClusterNightMode(true)));
        let night = obj(json!({ "nightMode": true, "clusterNightMode": true }));
        t.follow(&dark_cluster, &night, &cp);
        assert_eq!(cmds.try_recv(), Ok(CpCmd::NightMode(true)));
        assert!(cmds.try_recv().is_err(), "the cluster keeps its own");
        let cleared = obj(json!({ "nightMode": true, "clusterNightMode": null }));
        let lit_cluster = obj(json!({ "nightMode": true, "clusterNightMode": false }));
        t.follow(&night, &lit_cluster, &cp);
        assert_eq!(cmds.try_recv(), Ok(CpCmd::ClusterNightMode(false)));
        t.follow(&lit_cluster, &cleared, &cp);
        assert_eq!(
            cmds.try_recv(),
            Ok(CpCmd::ClusterNightMode(true)),
            "back to nightMode"
        );
    }

    #[test]
    fn a_notification_opens_with_its_fields_and_closes() {
        let (tx, mut cmds) = mpsc::unbounded_channel();
        let cp = CpHandle::from_sender(tx);
        let mut t = CpTelemetry::new(HelperSock::default());
        let open = obj(json!({ "notification": {
            "title": "Frost", "text": "Glätte möglich", "symbol": "snowflake", "open": true
        } }));
        t.follow(&Map::new(), &open, &cp);
        let frost = Notice {
            title: "Frost".into(),
            text: "Glätte möglich".into(),
            symbol: "snowflake".into(),
        };
        assert_eq!(
            cmds.try_recv(),
            Ok(CpCmd::Notification(Some(frost.clone())))
        );
        t.follow(&open, &open, &cp);
        assert!(cmds.try_recv().is_err(), "unchanged, nothing new");

        let closed = obj(json!({ "notification": { "title": "Frost", "open": false } }));
        t.follow(&open, &closed, &cp);
        assert_eq!(cmds.try_recv(), Ok(CpCmd::Notification(None)));

        t.hydrate(&open, &cp);
        let reopened = std::iter::from_fn(|| cmds.try_recv().ok())
            .any(|c| c == CpCmd::Notification(Some(frost.clone())));
        assert!(reopened, "a new session shows the open notification");
    }

    #[test]
    fn climate_set_outside_the_phone_reaches_it_and_a_new_session_starts_from_the_store() {
        let (tx, mut cmds) = mpsc::unbounded_channel();
        let cp = CpHandle::from_sender(tx);
        let mut t = CpTelemetry::new(HelperSock::default());
        let first = obj(json!({ "climate": { "auto": true, "fanLevel": 3, "source": "default" } }));
        let second =
            obj(json!({ "climate": { "auto": true, "fanLevel": 5, "source": "external" } }));
        t.follow(&first, &second, &cp);
        assert_eq!(
            cmds.try_recv(),
            Ok(CpCmd::Climate(obj(json!({ "fanLevel": 5 }))))
        );

        let third =
            obj(json!({ "climate": { "auto": false, "fanLevel": 5, "source": "carplay" } }));
        t.follow(&second, &third, &cp);
        assert!(
            cmds.try_recv().is_err(),
            "the phone's own setting does not go back to it"
        );
        let renamed = obj(json!({ "climate": { "auto": false, "fanLevel": 5, "source": "car" } }));
        t.follow(&third, &renamed, &cp);
        assert!(cmds.try_recv().is_err());

        t.hydrate(&third, &cp);
        assert_eq!(
            cmds.try_recv(),
            Ok(CpCmd::Climate(obj(json!({ "auto": false, "fanLevel": 5 }))))
        );
        assert!(cmds.try_recv().is_err());
    }

    #[test]
    fn park_and_neutral_lift_the_limited_ui_and_only_park_allows_video() {
        let (tx, mut cmds) = mpsc::unbounded_channel();
        let cp = CpHandle::from_sender(tx);
        let mut t = CpTelemetry::new(HelperSock::default());
        let mut prev = Map::new();
        let mut step = |next: Value| {
            let next = obj(next);
            t.follow(&prev, &next, &cp);
            prev = next;
        };

        step(json!({ "gear": "x" }));
        step(json!({ "reverse": false }));
        assert!(cmds.try_recv().is_err());

        step(json!({ "gear": "p" }));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::LimitedUi(false)));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::VideoAllowed(true)));
        step(json!({ "gear": "D" }));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::LimitedUi(true)));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::VideoAllowed(false)));
        step(json!({ "gear": 3 }));
        step(json!({ "reverse": true }));
        assert!(cmds.try_recv().is_err());
        step(json!({ "gear": 0 }));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::LimitedUi(false)));
        step(json!({ "gear": "N" }));
        assert!(cmds.try_recv().is_err());

        t.hydrate(&obj(json!({ "gear": "N" })), &cp);
        assert_eq!(cmds.try_recv(), Ok(CpCmd::LimitedUi(false)));
        assert_eq!(cmds.try_recv(), Ok(CpCmd::VideoAllowed(false)));
        assert!(cmds.try_recv().is_err());
    }
}
