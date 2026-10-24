use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use livi_core_proto::config::{AppearanceMode, CarType, Config, EvConnectorType};
use livi_core_proto::state::State;
use serde_json::{Map, Value, json};
use socketioxide::SocketIo;
use socketioxide::extract::{Data, SocketRef};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tower_http::cors::CorsLayer;

use crate::server::Core;
use crate::state_files::Saved;

pub type Snapshot = Map<String, Value>;

pub const PORT: u16 = 4000;
const UPDATE: &str = "telemetry:update";
const PUSH: &str = "telemetry:push";
pub const HTTP_PUSH: &str = "/telemetry";
pub const CLIMATE: &str = "climate";
pub const NOTIFICATION: &str = "notification";
pub const SOURCE: &str = "source";
const FROM_OUTSIDE: &str = "external";
const FROM_DEFAULTS: &str = "default";
const FROM_LAST_RUN: &str = "saved";
const RENAMED: [(&str, &str); 1] = [("turn", "turnSignal")];
const NESTED: [&str; 5] = ["gps", "can", CLIMATE, "tirePressure", NOTIFICATION];
const VOLUME_STEP: f64 = 0.005;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

pub fn merge(snap: &mut Snapshot, patch: &Value) -> Option<Snapshot> {
    let patch = patch.as_object().filter(|p| !p.is_empty())?;
    let mut changed = Snapshot::new();
    for (key, incoming) in patch {
        let key = match RENAMED.iter().find(|(old, _)| old == key) {
            Some((_, new)) if patch.contains_key(*new) => continue,
            Some((_, new)) => *new,
            None => key.as_str(),
        };
        let incoming = &with_source(key, incoming);
        let merged = match (incoming, snap.get(key)) {
            (Value::Object(inc), Some(Value::Object(prev))) if NESTED.contains(&key) => {
                let mut block = prev.clone();
                block.extend(inc.clone());
                Value::Object(block)
            }
            _ => incoming.clone(),
        };
        snap.insert(key.to_string(), merged.clone());
        changed.insert(key.to_string(), merged);
    }
    if !patch.contains_key("ts") {
        let ts = Value::from(now_ms());
        snap.insert("ts".into(), ts.clone());
        changed.insert("ts".into(), ts);
    }
    Some(changed)
}

fn with_source(key: &str, incoming: &Value) -> Value {
    match incoming {
        Value::Object(block) if key == CLIMATE && !block.contains_key(SOURCE) => {
            let mut block = block.clone();
            block.insert(SOURCE.into(), FROM_OUTSIDE.into());
            Value::Object(block)
        }
        _ => incoming.clone(),
    }
}

fn night_of(mode: AppearanceMode) -> Option<bool> {
    match mode {
        AppearanceMode::Night => Some(true),
        AppearanceMode::Day => Some(false),
        AppearanceMode::Auto => None,
    }
}

fn valid(lat: f64, lng: f64) -> bool {
    lat.is_finite()
        && lng.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lng)
        && !(lat == 0.0 && lng == 0.0)
}

fn last_fix(gps: &Map<String, Value>) -> Option<Value> {
    let num = |k: &str| gps.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    let (lat, lng) = (num("lat")?, num("lng")?);
    if !valid(lat, lng) {
        return None;
    }
    let mut fix = json!({ "lat": lat, "lng": lng });
    for key in ["alt", "heading", "fixTs"] {
        if let Some(v) = gps.get(key).filter(|v| v.is_number()) {
            fix[key] = v.clone();
        }
    }
    Some(fix)
}

struct Telemetry {
    core: Arc<Core>,
    io: SocketIo,
    snap: Snapshot,
    appearance: AppearanceMode,
    dark: bool,
    volume: Option<f64>,
}

impl Telemetry {
    fn new(core: Arc<Core>, io: SocketIo, saved: &Saved) -> Self {
        let cfg = core.hub.config();
        let mut t = Self {
            core,
            io,
            snap: Snapshot::new(),
            appearance: cfg.appearance_mode,
            dark: cfg.dark_mode,
            volume: None,
        };
        t.seed(&cfg, saved);
        t
    }

    fn seed(&mut self, cfg: &Config, saved: &Saved) {
        let mut patch = Snapshot::new();
        if let Some(night) = night_of(cfg.appearance_mode) {
            patch.insert("nightMode".into(), night.into());
        }
        if let Some(fix) = saved.gps.as_ref().and_then(last_fix) {
            patch.insert("gps".into(), fix);
        }
        if let Some(vehicle) = &saved.vehicle {
            patch.extend(vehicle.clone());
        }
        if cfg.hu_volume.is_finite() {
            let level = cfg.hu_volume.clamp(0.0, 1.0);
            patch.insert("volume".into(), level.into());
            self.volume = Some(level);
        }
        let mut climate = livi_cp::vehicle_state::default_climate();
        let source = match &saved.climate {
            Some(last) => {
                climate.extend(last.clone());
                FROM_LAST_RUN
            }
            None => FROM_DEFAULTS,
        };
        climate.insert(SOURCE.into(), source.into());
        patch.insert(CLIMATE.into(), climate.into());
        merge(&mut self.snap, &Value::Object(patch));
    }

    async fn publish(&self) {
        let snap = self.snap.clone();
        self.core.hub.update(|s| s.telemetry = snap);
        let _ = self.io.emit(UPDATE, &self.snap).await;
    }

    async fn push(&mut self, patch: &Value) {
        let path = self.snap.get("path").cloned();
        let Some(changed) = merge(&mut self.snap, patch) else {
            return;
        };
        self.publish().await;
        if let Some(page) = self.snap.get("path").filter(|p| Some(*p) != path.as_ref())
            && let Some(page) = page.as_str()
        {
            let _ = self.core.open_page.send(page.to_string());
        }
        if let Some(level) = changed.get("volume").and_then(Value::as_f64) {
            self.volume_from(level).await;
        }
        if let Some(dark) = changed.get("darkMode").and_then(Value::as_bool) {
            self.dark_from(dark).await;
        }
        if let Some(name) = changed.get("carType").and_then(Value::as_str) {
            self.car_type_from(name).await;
        }
        if let Some(names) = changed.get("evConnectorTypes").and_then(Value::as_array) {
            self.connectors_from(names).await;
        }
        for (key, now) in [
            ("evMaxAcPowerKw", self.core.hub.config().ev_max_ac_power_kw),
            ("evMaxDcPowerKw", self.core.hub.config().ev_max_dc_power_kw),
        ] {
            if let Some(kw) = changed
                .get(key)
                .and_then(Value::as_f64)
                .filter(|kw| *kw > 0.0)
                && now != Some(kw)
            {
                self.save(json!({ key: kw })).await;
            }
        }
    }

    async fn appearance(&mut self, mode: AppearanceMode) {
        if mode == self.appearance {
            return;
        }
        self.appearance = mode;
        if let Some(night) = night_of(mode) {
            self.push(&json!({ "nightMode": night, "clusterNightMode": null }))
                .await;
        }
    }

    async fn dark_mode(&mut self, dark: bool) {
        if dark == self.dark {
            return;
        }
        self.dark = dark;
        self.push(&json!({ "darkMode": dark })).await;
    }

    async fn dark_from(&mut self, dark: bool) {
        self.dark = dark;
        if self.core.hub.config().dark_mode != dark {
            self.save(json!({ "darkMode": dark })).await;
        }
    }

    async fn volume_from(&mut self, raw: f64) {
        if !raw.is_finite() {
            return;
        }
        let level = raw.clamp(0.0, 1.0);
        if self
            .volume
            .is_some_and(|last| (level - last).abs() < VOLUME_STEP)
        {
            return;
        }
        self.volume = Some(level);
        println!(
            "[volume] head unit set to {} % from telemetry",
            (level * 100.0).round()
        );
        self.save(json!({ "huVolume": level })).await;
    }

    async fn car_type_from(&self, name: &str) {
        let Some(car) = CarType::from_name(name) else {
            return;
        };
        if self.core.hub.config().car_type == Some(car) {
            return;
        }
        println!("[telemetry] car type {name} from the car");
        self.save(json!({ "carType": car })).await;
    }

    async fn connectors_from(&self, names: &[Value]) {
        let connectors: Vec<EvConnectorType> = names
            .iter()
            .filter_map(Value::as_str)
            .filter_map(EvConnectorType::from_name)
            .collect();
        if connectors.is_empty()
            || self.core.hub.config().ev_connector_types == Some(connectors.clone())
        {
            return;
        }
        self.save(json!({ "evConnectorTypes": connectors })).await;
    }

    async fn save(&self, patch: Value) {
        if let Err(e) = self.core.set_config(&patch).await {
            eprintln!("[telemetry] config not saved: {e}");
        }
    }
}

pub fn socket(
    state: watch::Receiver<State>,
) -> (axum::Router, SocketIo, mpsc::UnboundedReceiver<Value>) {
    let (service, io) = SocketIo::new_svc();
    let (pushes, pushed) = mpsc::unbounded_channel();
    let over_http = pushes.clone();
    io.ns("/", move |socket: SocketRef| {
        let snap = state.borrow().telemetry.clone();
        if !snap.is_empty() {
            let _ = socket.emit(UPDATE, &snap);
        }
        let pushes = pushes.clone();
        socket.on(PUSH, move |Data::<Value>(payload)| {
            let _ = pushes.send(payload);
            async {}
        });
        async {}
    });
    let http_push = move |body: axum::body::Bytes| {
        let pushes = over_http.clone();
        async move {
            match serde_json::from_slice::<Value>(&body) {
                Ok(patch @ Value::Object(_)) => {
                    let _ = pushes.send(patch);
                    StatusCode::NO_CONTENT
                }
                _ => StatusCode::BAD_REQUEST,
            }
        }
    };
    let app = axum::Router::new()
        .route(HTTP_PUSH, axum::routing::post(http_push))
        .fallback_service(service)
        .layer(CorsLayer::permissive());
    (app, io, pushed)
}

async fn listen() -> Option<TcpListener> {
    for addr in [format!("[::]:{PORT}"), format!("0.0.0.0:{PORT}")] {
        match TcpListener::bind(&addr).await {
            Ok(listener) => return Some(listener),
            Err(e) => eprintln!("[telemetry] cannot listen on {addr}: {e}"),
        }
    }
    None
}

pub async fn run(core: Arc<Core>, mut inside: mpsc::UnboundedReceiver<Value>, saved: Saved) {
    let (app, io, mut pushed) = socket(core.hub.watch());
    tokio::spawn(async move {
        let Some(listener) = listen().await else {
            return;
        };
        println!("[telemetry] listening on port {PORT}");
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[telemetry] server stopped: {e}");
        }
    });
    let mut t = Telemetry::new(core.clone(), io, &saved);
    t.publish().await;
    let mut state = core.hub.watch();
    loop {
        tokio::select! {
            Some(patch) = pushed.recv() => t.push(&patch).await,
            Some(patch) = inside.recv() => t.push(&patch).await,
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                let (mode, dark) = {
                    let cfg = &state.borrow_and_update().config;
                    (cfg.appearance_mode, cfg.dark_mode)
                };
                t.appearance(mode).await;
                t.dark_mode(dark).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::process::ExitCode;
    use std::time::Duration;

    use livi_core_proto::state::{Front, PerScreen};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::Notify;

    use super::*;
    use crate::config_file::tests::TempDir;
    use crate::config_file::{ConfigFile, defaults};
    use crate::hub::Hub;

    fn state(config: Config, telemetry: Snapshot) -> State {
        State {
            front: PerScreen {
                main: Front::Livi,
                cluster: Front::Livi,
                aux: Front::Livi,
            },
            sessions: Default::default(),
            now_playing: Default::default(),
            telemetry,
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            cluster: Default::default(),
            dab: Default::default(),
            cameras: Vec::new(),
            mfi: Default::default(),
            config,
        }
    }

    fn core(dir: &TempDir, config: Config) -> Arc<Core> {
        let livi = PerScreen {
            main: Front::Livi,
            cluster: Front::Livi,
            aux: Front::Livi,
        };
        let hub = Hub::new(state(config, Snapshot::new()));
        let file = ConfigFile::new(dir.0.join("config.json"), dir.0.join("backup.json"));
        let (senders, ..) = crate::projection::ui_channels(livi);
        Arc::new(Core::new(hub, file, 0, Arc::new(Notify::new()), senders))
    }

    fn obj(v: Value) -> Snapshot {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn gps_and_can_merge_by_field_everything_else_is_replaced() {
        let mut snap = obj(json!({ "gps": { "lat": 1.0, "lng": 2.0 }, "gear": "D", "ts": 5 }));
        let changed = merge(&mut snap, &json!({ "gps": { "lat": 3.0 }, "gear": 2 })).unwrap();
        assert_eq!(snap["gps"], json!({ "lat": 3.0, "lng": 2.0 }));
        assert_eq!(
            (&snap["gear"], &changed["gps"]),
            (&json!(2), &json!({ "lat": 3.0, "lng": 2.0 }))
        );
        assert!(changed["ts"].as_u64().unwrap() > 5);
        assert_eq!(snap["ts"], changed["ts"]);

        merge(&mut snap, &json!({ "turn": "left" }));
        assert_eq!(
            (&snap["turnSignal"], snap.get("turn")),
            (&json!("left"), None)
        );
        merge(&mut snap, &json!({ "turn": "left", "turnSignal": "none" }));
        assert_eq!(snap["turnSignal"], "none", "the current name wins");

        merge(
            &mut snap,
            &json!({ "tirePressure": { "frontLeftKpa": 230 } }),
        );
        merge(
            &mut snap,
            &json!({ "tirePressure": { "rearRightKpa": 210 } }),
        );
        assert_eq!(
            snap["tirePressure"],
            json!({ "frontLeftKpa": 230, "rearRightKpa": 210 })
        );
        merge(
            &mut snap,
            &json!({ "notification": { "title": "Frost", "open": true } }),
        );
        merge(&mut snap, &json!({ "notification": { "open": false } }));
        assert_eq!(
            snap["notification"],
            json!({ "title": "Frost", "open": false })
        );

        let changed = merge(&mut snap, &json!({ "can": { "id": 1 }, "ts": 7 })).unwrap();
        assert_eq!(
            (&snap["ts"], &changed["can"]),
            (&json!(7), &json!({ "id": 1 }))
        );
        assert!(merge(&mut snap, &json!({})).is_none());
        assert!(merge(&mut snap, &json!([1])).is_none());
        merge(&mut snap, &json!({ "gps": "lost" }));
        assert_eq!(snap["gps"], "lost");
    }

    #[test]
    fn climate_merges_by_field_and_names_who_set_it() {
        let mut snap = Snapshot::new();
        merge(
            &mut snap,
            &json!({ "climate": { "auto": true, "fanLevel": 3, "source": "carplay" } }),
        );
        merge(&mut snap, &json!({ "climate": { "fanLevel": 5 } }));
        assert_eq!(
            snap["climate"],
            json!({ "auto": true, "fanLevel": 5, "source": "external" })
        );
        merge(
            &mut snap,
            &json!({ "climate": { "fanLevel": 4, "source": "car" } }),
        );
        assert_eq!(snap["climate"]["source"], "car");
    }

    #[tokio::test]
    async fn the_config_and_the_last_run_seed_the_snapshot() {
        let dir = TempDir::new();
        let mut cfg = defaults();
        cfg.appearance_mode = AppearanceMode::Night;
        cfg.hu_volume = 0.5;
        let core = core(&dir, cfg);
        let (_, io, _) = socket(core.hub.watch());
        let saved = Saved {
            climate: obj(json!({ "auto": false, "driverTargetC": 19.0, "source": "carplay" }))
                .into(),
            gps: obj(
                json!({ "lat": 48.1, "lng": 11.5, "alt": 520.0, "speedMs": 30.0, "fixTs": 9 }),
            )
            .into(),
            vehicle: obj(json!({ "fuelPct": 42, "odometerKm": 123456 })).into(),
            ..Saved::default()
        };
        let mut t = Telemetry::new(core.clone(), io, &saved);
        assert_eq!(t.snap["nightMode"], true);
        assert!(t.snap.get("clusterNightMode").is_none());
        assert_eq!(
            (&t.snap["fuelPct"], &t.snap["odometerKm"]),
            (&json!(42), &json!(123456))
        );
        assert_eq!(t.snap["volume"], 0.5);
        assert_eq!(
            t.snap["gps"],
            json!({ "lat": 48.1, "lng": 11.5, "alt": 520.0, "fixTs": 9 })
        );
        let climate = &t.snap["climate"];
        assert_eq!(
            (
                &climate["auto"],
                &climate["driverTargetC"],
                &climate["fanLevel"]
            ),
            (&json!(false), &json!(19.0), &json!(3)),
            "the last run's values over LIVI's own"
        );
        assert_eq!(climate["source"], "saved");

        t.push(&json!({ "volume": 0.502 })).await;
        assert_eq!(core.hub.config().hu_volume, 0.5);
        t.push(&json!({ "volume": 2.0 })).await;
        assert_eq!(core.hub.config().hu_volume, 1.0);
        assert_eq!(core.hub.watch().borrow().telemetry["volume"], 2.0);

        t.appearance(AppearanceMode::Night).await;
        t.appearance(AppearanceMode::Day).await;
        let both = core.hub.watch().borrow().telemetry.clone();
        assert_eq!(
            (&both["nightMode"], &both["clusterNightMode"]),
            (&json!(false), &Value::Null)
        );
        t.appearance(AppearanceMode::Auto).await;
        assert_eq!(t.snap["nightMode"], false);
    }

    #[tokio::test]
    async fn a_new_path_opens_its_page_once() {
        let dir = TempDir::new();
        let core = core(&dir, defaults());
        let mut pages = core.open_page.subscribe();
        let (_, io, _) = socket(core.hub.watch());
        let mut t = Telemetry::new(core.clone(), io, &Saved::default());
        t.push(&json!({ "path": "/media" })).await;
        t.push(&json!({ "path": "/media", "speedKph": 3 })).await;
        t.push(&json!({ "path": 5 })).await;
        t.push(&json!({ "path": "/settings" })).await;
        assert_eq!(pages.try_recv(), Ok("/media".into()));
        assert_eq!(pages.try_recv(), Ok("/settings".into()));
        assert!(pages.try_recv().is_err());
    }

    #[test]
    fn livis_climate_is_what_the_contract_names() {
        let climate = Value::Object(livi_cp::vehicle_state::default_climate());
        let named: livi_core_proto::telemetry::Climate = serde_json::from_value(climate).unwrap();
        let back = serde_json::to_value(&named).unwrap();
        let unsent: Vec<&String> = back
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, v)| v.is_null() && *key != SOURCE)
            .map(|(key, _)| key)
            .collect();
        assert!(
            unsent.is_empty(),
            "the contract names {unsent:?}, LIVI never sends them"
        );
    }

    #[tokio::test]
    async fn the_car_type_and_connectors_from_the_car_go_into_the_config() {
        let dir = TempDir::new();
        let core = core(&dir, defaults());
        let (_, io, _) = socket(core.hub.watch());
        let mut t = Telemetry::new(core.clone(), io, &Saved::default());

        t.push(&json!({ "carType": "hybridGasoline" })).await;
        assert_eq!(core.hub.config().car_type, Some(CarType::HybridGasoline));
        t.push(&json!({ "carType": "Steam" })).await;
        t.push(&json!({ "carType": 10 })).await;
        assert_eq!(core.hub.config().car_type, Some(CarType::HybridGasoline));

        t.push(&json!({ "evConnectorTypes": ["Combo2", "Type 2", "mennekes"] }))
            .await;
        let both = Some(vec![EvConnectorType::Combo2, EvConnectorType::Mennekes]);
        assert_eq!(core.hub.config().ev_connector_types, both);
        t.push(&json!({ "evConnectorTypes": ["Type 2"] })).await;
        t.push(&json!({ "evConnectorTypes": ["Combo2", "Mennekes"] }))
            .await;
        assert_eq!(core.hub.config().ev_connector_types, both);

        t.push(&json!({ "evMaxAcPowerKw": 11, "evMaxDcPowerKw": 0 }))
            .await;
        assert_eq!(core.hub.config().ev_max_ac_power_kw, Some(11.0));
        assert_eq!(
            core.hub.config().ev_max_dc_power_kw,
            None,
            "zero is no power"
        );
        t.push(&json!({ "evMaxDcPowerKw": 150.5 })).await;
        assert_eq!(core.hub.config().ev_max_dc_power_kw, Some(150.5));
    }

    #[tokio::test]
    async fn dark_mode_from_the_car_sets_livis_look_and_follows_the_setting() {
        let dir = TempDir::new();
        let core = core(&dir, defaults());
        let (_, io, _) = socket(core.hub.watch());
        let mut t = Telemetry::new(core.clone(), io, &Saved::default());
        let dark = core.hub.config().dark_mode;

        t.push(&json!({ "darkMode": !dark })).await;
        assert_eq!(core.hub.config().dark_mode, !dark);

        core.set_config(&json!({ "darkMode": dark })).await.unwrap();
        t.dark_mode(dark).await;
        assert_eq!(t.snap["darkMode"], json!(dark));

        t.push(&json!({ "darkMode": !dark })).await;
        assert_eq!(core.hub.config().dark_mode, !dark, "the car wins again");
        t.push(&json!({ "darkMode": "yes" })).await;
        assert_eq!(core.hub.config().dark_mode, !dark);
    }

    #[tokio::test]
    async fn nothing_known_leaves_only_the_climate_livi_starts_with() {
        let dir = TempDir::new();
        let mut cfg = defaults();
        cfg.hu_volume = f64::NAN;
        let core = core(&dir, cfg);
        let (_, io, _) = socket(core.hub.watch());
        let saved = Saved {
            gps: obj(json!({ "lat": 0.0, "lng": 0.0 })).into(),
            climate: None,
            vehicle: None,
            ..Saved::default()
        };
        let mut t = Telemetry::new(core, io, &saved);
        let keys: Vec<&str> = t.snap.keys().map(String::as_str).collect();
        assert_eq!(keys, ["climate", "ts"]);
        t.volume_from(f64::INFINITY).await;
        assert_eq!(t.volume, None);
    }

    async fn poll(addr: SocketAddr, method: &str, sid: &str, body: &str) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        let sid = if sid.is_empty() {
            String::new()
        } else {
            format!("&sid={sid}")
        };
        let req = format!(
            "{method} /socket.io/?EIO=4&transport=polling{sid} HTTP/1.1\r\nHost: livi\r\n\
             Connection: close\r\nContent-Type: text/plain;charset=UTF-8\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        tokio::time::timeout(Duration::from_secs(5), s.read_to_string(&mut out))
            .await
            .unwrap()
            .unwrap();
        out
    }

    #[tokio::test]
    async fn a_push_over_plain_http_arrives_like_one_over_socket_io() {
        let (_tx, rx) = watch::channel(state(defaults(), Snapshot::new()));
        let (app, _io, mut pushed) = socket(rx);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let to = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await });
        let shell = |args: &'static str, to: &str| {
            let mut words: Vec<String> = args.split_whitespace().map(str::to_string).collect();
            words.extend(["--to".into(), to.to_string()]);
            tokio::task::spawn_blocking(move || crate::telemetry_cli::run(&words))
        };

        assert_eq!(
            shell("set gear=R gps.lat=53.59", &to).await.unwrap(),
            ExitCode::SUCCESS
        );
        assert_eq!(
            pushed.recv().await,
            Some(json!({ "gear": "R", "gps": { "lat": 53.59 } }))
        );
        let posted = to.clone();
        let refused =
            tokio::task::spawn_blocking(move || crate::telemetry_cli::post(&posted, &json!([1])));
        assert!(refused.await.unwrap().unwrap_err().contains("400"));
        assert!(pushed.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_socket_io_client_gets_the_snapshot_and_its_pushes_arrive() {
        let snap = obj(json!({ "speedKph": 42 }));
        let (_tx, rx) = watch::channel(state(defaults(), snap));
        let (app, io, mut pushed) = socket(rx);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });

        let open = poll(addr, "GET", "", "").await;
        let sid = open
            .split("\"sid\":\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap();
        let joining = poll(addr, "POST", sid, "40").await;
        assert!(joining.contains("ok"), "{open}\n---\n{joining}");
        let joined = poll(addr, "GET", sid, "").await;
        assert!(joined.contains("40{\"sid\""));
        assert!(joined.contains(r#"42["telemetry:update",{"speedKph":42}]"#));

        poll(addr, "POST", sid, r#"42["telemetry:push",{"rpm":900}]"#).await;
        let push = tokio::time::timeout(Duration::from_secs(5), pushed.recv())
            .await
            .unwrap();
        assert_eq!(push, Some(json!({ "rpm": 900 })));

        io.emit(UPDATE, &json!({ "rpm": 900 })).await.unwrap();
        assert!(
            poll(addr, "GET", sid, "")
                .await
                .contains(r#"42["telemetry:update",{"rpm":900}]"#)
        );
        let elsewhere = poll(addr, "GET", "", "").await;
        assert!(
            elsewhere.contains("Access-Control-Allow-Origin")
                || elsewhere.contains("access-control-allow-origin")
        );
    }
}
