//! The settings file, shared with the Electron app.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use livi_core_proto::config::Config;
pub use livi_core_proto::config::defaults;
use serde_json::{Map, Value};

/// carName is the Wi-Fi AP, Bluetooth and head-unit name.
pub const CAR_NAME_MAX: usize = 20;
/// What hostapd accepts as a WPA passphrase.
const WIFI_PASSWORD_LEN: std::ops::RangeInclusive<usize> = 8..=63;
const OMIT_WHEN_EMPTY: [&str; 3] = ["carplayIcon120", "carplayIcon180", "carplayIcon256"];

#[derive(Default)]
pub struct HostFacts {
    pub car_name: Option<String>,
    pub panel_px: Option<(u32, u32)>,
}

pub struct ConfigFile {
    path: PathBuf,
    backup: PathBuf,
}

impl ConfigFile {
    pub fn new(path: PathBuf, backup: PathBuf) -> Self {
        Self { path, backup }
    }

    pub fn load(&self, host: &HostFacts) -> Config {
        let source = [&self.path, &self.backup].into_iter().find(|p| p.exists());
        let file = source.and_then(|p| read_json(p)).unwrap_or(Value::Object(Map::new()));
        if source == Some(&self.backup) {
            println!("[core] config restored from the backup mirror");
        }

        let mut defaults = defaults();
        if file.get("carName").is_none()
            && let Some(name) = &host.car_name
        {
            defaults.car_name = name.clone();
        }
        if file.get("projectionWidth").is_none()
            && file.get("projectionHeight").is_none()
            && let Some((w, h)) = host.panel_px
        {
            let (w, h) = fit_720p(w, h);
            println!("[core] projection defaults from the panel: {w}x{h}");
            defaults.projection_width = w;
            defaults.projection_height = h;
            defaults.cluster_width = w;
            defaults.cluster_height = h;
        }

        let mut config = merge(&current_names(&file), &defaults);
        if !WIFI_PASSWORD_LEN.contains(&config.wifi_password.chars().count()) {
            println!("[core] wifiPassword is no WPA passphrase, falling back to the default");
            config.wifi_password = self::defaults().wifi_password;
        }

        if !self.path.exists() || file != for_file(&config) {
            match self.save(&config) {
                Ok(()) => println!("[core] wrote the corrected config"),
                Err(e) => eprintln!("[core] cannot write {}: {e}", self.path.display()),
            }
        }
        config
    }

    pub fn save(&self, config: &Config) -> io::Result<()> {
        let json = serde_json::to_string_pretty(&for_file(config))?;
        write_atomic(&self.path, &json)?;
        if let Err(e) = write_atomic(&self.backup, &json) {
            eprintln!("[core] config backup mirror failed: {e}");
        }
        Ok(())
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("[core] {} is no valid JSON, using the defaults: {e}", path.display());
            None
        }
    }
}

fn write_atomic(path: &Path, data: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

fn for_file(config: &Config) -> Value {
    let mut value = serde_json::to_value(config).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        for key in OMIT_WHEN_EMPTY {
            if map.get(key).and_then(Value::as_str).is_some_and(|s| s.trim().is_empty()) {
                map.remove(key);
            }
        }
    }
    value
}

/// Keys of earlier releases under their current names.
fn current_names(file: &Value) -> Value {
    let mut file = file.clone();
    let Value::Object(top) = &mut file else { return file };
    for (old, new) in [
        ("dashScreenBounds", "clusterScreenBounds"),
        ("dashScreenActive", "clusterScreenActive"),
        ("dashScreenWidth", "clusterScreenWidth"),
        ("dashScreenHeight", "clusterScreenHeight"),
    ] {
        rename(top, old, new);
    }
    for per_screen in ["kiosk", "camera", "media", "custom"] {
        if let Some(Value::Object(screens)) = top.get_mut(per_screen) {
            rename(screens, "dash", "cluster");
        }
    }
    if let Some(Value::Object(slots)) = top.get_mut("dashboards") {
        for i in 1..=4 {
            let slot = format!("dashboard{i}");
            rename(slots, &format!("dash{i}"), &slot);
            if let Some(Value::Object(screens)) = slots.get_mut(&slot) {
                rename(screens, "dash", "cluster");
            }
        }
    }
    file
}

fn rename(map: &mut Map<String, Value>, old: &str, new: &str) {
    if let Some(value) = map.remove(old)
        && !map.contains_key(new)
    {
        map.insert(new.to_string(), value);
    }
}

/// Every phone handles 1280x720.
fn fit_720p(width: u32, height: u32) -> (u32, u32) {
    let scale = (1280.0 / f64::from(width)).min(720.0 / f64::from(height)).min(1.0);
    let even = |v: u32| 2 * (f64::from(v) * scale / 2.0).round() as u32;
    (even(width), even(height))
}

fn merge(file: &Value, defaults: &Config) -> Config {
    let schema = serde_json::to_value(defaults).unwrap_or(Value::Null);
    let mut merged = keep_typed(file, &schema);
    if let (Value::Object(out), Value::Object(src), Value::Object(known)) =
        (&mut merged, file, &schema)
    {
        for (key, value) in src {
            if !known.contains_key(key) {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    if let Ok(config) = serde_json::from_value(merged.clone()) {
        return config;
    }
    let (Value::Object(fields), Value::Object(known)) = (&mut merged, &schema) else {
        return defaults.clone();
    };
    let keys: Vec<String> = fields.keys().cloned().collect();
    for key in keys {
        let mut alone = known.clone();
        alone.insert(key.clone(), fields[&key].clone());
        if serde_json::from_value::<Config>(Value::Object(alone)).is_err() {
            match known.get(&key) {
                Some(def) => fields.insert(key, def.clone()),
                None => fields.remove(&key),
            };
        }
    }
    serde_json::from_value(merged).unwrap_or_else(|_| defaults.clone())
}

fn keep_typed(input: &Value, schema: &Value) -> Value {
    let Value::Object(schema) = schema else { return schema.clone() };
    let source = input.as_object();
    let mut out = Map::new();
    for (key, def) in schema {
        let chosen = match (source.and_then(|s| s.get(key)), def) {
            (None, _) => def.clone(),
            (Some(v), Value::Object(_)) if v.is_object() => keep_typed(v, def),
            (Some(v), _) if same_type(v, def) => v.clone(),
            _ => def.clone(),
        };
        out.insert(key.clone(), chosen);
    }
    Value::Object(out)
}

fn same_type(a: &Value, b: &Value) -> bool {
    matches!(
        (a, b),
        (Value::Bool(_), Value::Bool(_))
            | (Value::Number(_), Value::Number(_))
            | (Value::String(_), Value::String(_))
            | (Value::Array(_), Value::Array(_))
    )
}

pub fn apply_patch(current: &Config, patch: &Value) -> Result<Config, String> {
    let Value::Object(patch) = patch else { return Err("the patch is no object".into()) };
    let mut value = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let Value::Object(fields) = &mut value else { return Err("config is no object".into()) };
    for (key, v) in patch {
        match (key.as_str(), v) {
            (_, Value::Null) => {
                fields.remove(key);
            }
            ("bindings", Value::Object(changed)) => {
                if let Some(Value::Object(bindings)) = fields.get_mut("bindings") {
                    bindings.extend(changed.clone());
                }
            }
            _ => {
                fields.insert(key.clone(), v.clone());
            }
        }
    }
    let next: Config = serde_json::from_value(value).map_err(|e| format!("rejected: {e}"))?;

    let written = serde_json::to_value(&next).map_err(|e| e.to_string())?;
    if let Some(unknown) = patch.iter().find(|(k, v)| !v.is_null() && written.get(*k).is_none()) {
        return Err(format!("unknown setting {}", unknown.0));
    }
    if next.wifi_password != current.wifi_password
        && !WIFI_PASSWORD_LEN.contains(&next.wifi_password.chars().count())
    {
        return Err("wifiPassword needs 8 to 63 characters".into());
    }
    Ok(next)
}

#[cfg(test)]
pub mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use livi_core_proto::state::PerScreen;
    use serde_json::json;

    use super::*;

    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("livi-core-test-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn file_in(dir: &TempDir) -> ConfigFile {
        ConfigFile::new(dir.0.join("LIVI/config.json"), dir.0.join("backup/config.json"))
    }

    fn written(dir: &TempDir, rel: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(dir.0.join(rel)).unwrap()).unwrap()
    }

    #[test]
    fn a_fresh_install_writes_the_defaults_and_their_mirror() {
        let dir = TempDir::new();
        let config = file_in(&dir).load(&HostFacts::default());
        assert_eq!(config, defaults());
        let live = written(&dir, "LIVI/config.json");
        assert_eq!(live, written(&dir, "backup/config.json"));
        assert_eq!(live["carName"], "LIVI");
        assert!(live.get("carplayIcon120").is_none());
    }

    #[test]
    fn a_fresh_install_takes_the_host_name_and_the_panel() {
        let dir = TempDir::new();
        let host = HostFacts { car_name: Some("golf".into()), panel_px: Some((1920, 1200)) };
        let config = file_in(&dir).load(&host);
        assert_eq!(config.car_name, "golf");
        assert_eq!((config.projection_width, config.projection_height), (1152, 720));
        assert_eq!((config.cluster_width, config.cluster_height), (1152, 720));
    }

    #[test]
    fn the_old_dash_names_come_over_and_are_written_under_the_new_ones() {
        let dir = TempDir::new();
        let bounds = json!({ "x": 8, "y": 31, "width": 1211, "height": 678 });
        let file = json!({
            "dashScreenActive": true,
            "dashScreenWidth": 1024,
            "dashScreenHeight": 600,
            "dashScreenBounds": bounds,
            "clusterScreenHeight": 480,
            "kiosk": { "main": false, "dash": true, "aux": false },
            "media": { "main": true, "dash": true, "aux": false },
            "dashboards": {
                "dash1": { "main": false, "dash": false, "aux": false, "pos": 1 },
                "dash2": { "main": false, "dash": true, "aux": false, "pos": 2 },
                "dash3": { "main": true, "dash": true, "aux": false, "pos": 3 },
                "dash4": { "main": false, "dash": false, "aux": true, "pos": 4 }
            }
        });
        fs::create_dir_all(dir.0.join("LIVI")).unwrap();
        fs::write(dir.0.join("LIVI/config.json"), file.to_string()).unwrap();

        let host = HostFacts { car_name: None, panel_px: None };
        let config = file_in(&dir).load(&host);
        assert!(config.cluster_screen_active);
        assert_eq!((config.cluster_screen_width, config.cluster_screen_height), (1024, 480));
        assert_eq!(config.cluster_screen_bounds.map(|b| b.width), Some(1211));
        assert_eq!(config.kiosk, PerScreen { main: false, cluster: true, aux: false });
        assert!(config.media.cluster);
        assert!(config.dashboards.dashboard2.cluster && config.dashboards.dashboard3.main);
        assert!(config.dashboards.dashboard4.aux && !config.dashboards.dashboard1.cluster);

        let live = written(&dir, "LIVI/config.json");
        let text = live.to_string();
        for old in ["\"dash\":", "\"dash1\":", "\"dash3\":", "\"dashScreen"] {
            assert!(!text.contains(old), "{old} is left: {text}");
        }
        assert_eq!(live["clusterScreenWidth"], 1024);
        assert_eq!(live["dashboards"]["dashboard3"]["cluster"], true);
    }

    #[test]
    fn values_of_the_wrong_type_fall_back_one_by_one() {
        let dir = TempDir::new();
        let file = json!({
            "carName": "Bulli",
            "huVolume": "loud",
            "wifiChannel": 36.5,
            "kiosk": { "main": true, "cluster": "yes" },
            "evConnectorTypes": [999],
            "lastKnownGps": { "lat": 52.5, "lng": 13.4, "ts": 1700000000000u64 },
            "dongleEra": true
        });
        fs::create_dir_all(dir.0.join("LIVI")).unwrap();
        fs::write(dir.0.join("LIVI/config.json"), file.to_string()).unwrap();

        let host = HostFacts { car_name: Some("ignored".into()), panel_px: Some((800, 480)) };
        let config = file_in(&dir).load(&host);
        assert_eq!(config.car_name, "Bulli");
        assert_eq!(config.hu_volume, 0.95);
        assert_eq!(config.wifi_channel, 36);
        assert_eq!(config.kiosk, PerScreen { main: true, cluster: false, aux: false });
        assert_eq!(config.ev_connector_types, Some(Vec::new()));
        assert_eq!((config.projection_width, config.projection_height), (800, 480));

        let live = written(&dir, "LIVI/config.json");
        assert!(live.get("dongleEra").is_none());
        assert!(live.get("lastKnownGps").is_none(), "the last position lives in state/gps.json");
    }

    #[test]
    fn broken_json_and_a_short_password_fall_back() {
        let dir = TempDir::new();
        fs::create_dir_all(dir.0.join("LIVI")).unwrap();
        fs::write(dir.0.join("LIVI/config.json"), "{ nope").unwrap();
        assert_eq!(file_in(&dir).load(&HostFacts::default()), defaults());

        fs::write(dir.0.join("LIVI/config.json"), json!({ "wifiPassword": "short" }).to_string())
            .unwrap();
        assert_eq!(file_in(&dir).load(&HostFacts::default()).wifi_password, "12345678");
    }

    #[test]
    fn a_missing_file_is_restored_from_the_mirror() {
        let dir = TempDir::new();
        fs::create_dir_all(dir.0.join("backup")).unwrap();
        fs::write(dir.0.join("backup/config.json"), json!({ "carName": "Restored" }).to_string())
            .unwrap();
        let config = file_in(&dir).load(&HostFacts::default());
        assert_eq!(config.car_name, "Restored");
        assert_eq!(written(&dir, "LIVI/config.json")["carName"], "Restored");
    }

    #[test]
    fn an_unchanged_file_is_left_alone() {
        let dir = TempDir::new();
        let file = file_in(&dir);
        file.load(&HostFacts::default());
        let path = dir.0.join("LIVI/config.json");
        fs::write(&path, serde_json::to_string(&for_file(&defaults())).unwrap()).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        file.load(&HostFacts::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn a_patch_replaces_fields_and_merges_bindings() {
        let next = apply_patch(
            &defaults(),
            &json!({ "huVolume": 0.5, "kiosk": { "main": true, "cluster": false, "aux": false },
                     "bindings": { "home": "KeyX" } }),
        )
        .unwrap();
        assert_eq!(next.hu_volume, 0.5);
        assert!(next.kiosk.main);
        assert_eq!(next.bindings.home, "KeyX");
        assert_eq!(next.bindings.play_pause, "KeyP");
    }

    #[test]
    fn null_clears_only_optional_fields() {
        let next = apply_patch(&defaults(), &json!({ "primaryColorDark": null })).unwrap();
        assert_eq!(next.primary_color_dark, None);
        assert!(apply_patch(&defaults(), &json!({ "carName": null })).is_err());
    }

    #[test]
    fn a_patch_that_does_not_fit_changes_nothing() {
        let current = defaults();
        assert!(apply_patch(&current, &json!({ "huVolume": "loud" })).is_err());
        assert!(apply_patch(&current, &json!({ "wifiChannel": 36.5 })).is_err());
        assert_eq!(
            apply_patch(&current, &json!({ "nightMode": true })),
            Err("unknown setting nightMode".into())
        );
        assert!(apply_patch(&current, &json!({ "wifiPassword": "short" })).is_err());
        assert!(apply_patch(&current, &json!([1])).is_err());
    }

    #[test]
    fn fit_720p_keeps_the_aspect_and_even_sizes() {
        assert_eq!(fit_720p(1920, 1080), (1280, 720));
        assert_eq!(fit_720p(1024, 600), (1024, 600));
        assert_eq!(fit_720p(1280, 800), (1152, 720));
    }
}
