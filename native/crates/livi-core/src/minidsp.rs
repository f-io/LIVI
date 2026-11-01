// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! MiniDSP control through the minidsp-rs daemon (`minidspd`) HTTP API.
//!
//! The page polls the device status while it is shown and after every action.
//! Volume and preset go to `master_status`, the bass control sets the gain of
//! the configured outputs.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::config::MiniDspConfig;
use livi_core_proto::state::MiniDsp;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::server::Core;

pub const TIMEOUT: Duration = Duration::from_secs(4);
const POLL: Duration = Duration::from_secs(5);
const PAGE: &str = "/minidsp";
const MOCK_ENV: &str = "LIVI_MOCK_MINIDSP";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MiniDspAsk {
    Refresh,
    SetVolume(f64),
    SetBass(f64),
    SelectPreset(u32),
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.min(max).max(min)
}

fn finite(value: f64, label: &str) -> Result<f64, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("{label} must be a finite number"))
    }
}

fn check_preset(config: &MiniDspConfig, preset: u32) -> Result<(), String> {
    if config.presets.iter().any(|p| p.index == preset) {
        Ok(())
    } else {
        Err(format!("MiniDSP preset {preset} is not configured"))
    }
}

fn disconnected(config: &MiniDspConfig) -> MiniDsp {
    MiniDsp {
        volume_db: config.volume_min_db,
        ..Default::default()
    }
}

pub struct Client {
    http: reqwest::Client,
    config: MiniDspConfig,
}

impl Client {
    pub fn new(http: reqwest::Client, config: MiniDspConfig) -> Self {
        Self { http, config }
    }

    pub fn http(timeout: Duration) -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_default()
    }

    fn base_url(&self) -> Result<String, String> {
        let base = self.config.server_url.trim().trim_end_matches('/');
        let lower = base.to_ascii_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            return Err("MiniDSP server URL must use http or https".into());
        }
        Ok(base.to_string())
    }

    fn endpoint(&self, path: &str, device_scoped: bool) -> Result<String, String> {
        let base = self.base_url()?;
        Ok(if device_scoped {
            format!("{base}/devices/{}{path}", self.config.device_index)
        } else {
            format!("{base}{path}")
        })
    }

    async fn request(
        &self,
        path: &str,
        body: Option<Value>,
        device_scoped: bool,
    ) -> Result<Value, String> {
        let url = self.endpoint(path, device_scoped)?;
        let req = match &body {
            Some(body) => self.http.post(&url).body(body.to_string()),
            None => self.http.get(&url),
        }
        .header("Content-Type", "application/json");
        let unreachable = |e: reqwest::Error| {
            if e.is_timeout() {
                "MiniDSP daemon did not respond in time".to_string()
            } else {
                format!(
                    "Cannot connect to MiniDSP daemon at {}. Make sure minidspd is running.",
                    self.base_url().unwrap_or_default()
                )
            }
        };
        let res = req.send().await.map_err(unreachable)?;
        let status = res.status();
        let text = res.text().await.map_err(unreachable)?;
        if !status.is_success() {
            let detail = text.trim();
            return Err(if detail.is_empty() {
                format!("MiniDSP returned {}", status.as_u16())
            } else {
                format!("MiniDSP returned {}: {detail}", status.as_u16())
            });
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| format!("MiniDSP returned invalid JSON: {e}"))
    }

    pub async fn status(&self) -> Result<MiniDsp, String> {
        let devices = self.request("/devices", None, false).await?;
        let index = self.config.device_index as usize;
        let Some(device) = devices.as_array().and_then(|d| d.get(index)) else {
            return Ok(disconnected(&self.config));
        };
        let status = self.request("", None, true).await?;
        let master = &status["master"];
        let Some(volume) = master["volume"].as_f64() else {
            return Err("MiniDSP returned an invalid device status".into());
        };
        let product = device["product_name"]
            .as_str()
            .or_else(|| status["product_name"].as_str());
        Ok(MiniDsp {
            connected: true,
            preset: master["preset"].as_u64().unwrap_or(0) as u32,
            source: match &master["source"] {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            },
            volume_db: volume,
            muted: master["mute"].as_bool().unwrap_or(false),
            product_name: product.map(str::to_string),
            ..Default::default()
        })
    }

    pub async fn set_volume(&self, volume_db: f64) -> Result<(), String> {
        let c = &self.config;
        let volume = clamp(
            finite(volume_db, "Volume")?,
            c.volume_min_db,
            c.volume_max_db,
        );
        self.apply(json!({ "master_status": { "volume": volume } }))
            .await
    }

    /// Sets every configured bass output and returns the gain it used.
    pub async fn set_bass(&self, gain_db: f64) -> Result<f64, String> {
        let c = &self.config;
        let gain = clamp(finite(gain_db, "Bass gain")?, c.bass_min_db, c.bass_max_db);
        let outputs: Vec<Value> = c
            .bass_output_channels
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|index| json!({ "index": index, "gain": gain }))
            .collect();
        if outputs.is_empty() {
            return Err("No MiniDSP bass output channels configured".into());
        }
        self.apply(json!({ "outputs": outputs })).await?;
        Ok(gain)
    }

    pub async fn select_preset(&self, preset: u32) -> Result<(), String> {
        check_preset(&self.config, preset)?;
        self.apply(json!({ "master_status": { "preset": preset } }))
            .await
    }

    async fn apply(&self, patch: Value) -> Result<(), String> {
        self.request("/config", Some(patch), true).await.map(|_| ())
    }
}

/// In-memory device for development builds.
pub struct Mock {
    volume_db: f64,
    preset: u32,
}

impl Mock {
    pub fn new(config: &MiniDspConfig) -> Self {
        Self {
            volume_db: clamp(-18.0, config.volume_min_db, config.volume_max_db),
            preset: config.presets.first().map_or(0, |p| p.index),
        }
    }

    fn status(&self) -> MiniDsp {
        MiniDsp {
            connected: true,
            preset: self.preset,
            source: "Mock source".into(),
            volume_db: self.volume_db,
            product_name: Some("Mock MiniDSP".into()),
            ..Default::default()
        }
    }

    fn act(&mut self, config: &MiniDspConfig, ask: MiniDspAsk) -> Result<Option<f64>, String> {
        match ask {
            MiniDspAsk::Refresh => Ok(None),
            MiniDspAsk::SetVolume(v) => {
                self.volume_db = clamp(
                    finite(v, "Volume")?,
                    config.volume_min_db,
                    config.volume_max_db,
                );
                Ok(None)
            }
            MiniDspAsk::SetBass(v) => Ok(Some(clamp(
                finite(v, "Bass gain")?,
                config.bass_min_db,
                config.bass_max_db,
            ))),
            MiniDspAsk::SelectPreset(p) => {
                check_preset(config, p)?;
                self.preset = p;
                Ok(None)
            }
        }
    }
}

fn mock_wanted(config: &MiniDspConfig) -> Result<bool, String> {
    let wanted = config.mock_device || std::env::var(MOCK_ENV).is_ok_and(|v| v == "1");
    if wanted && !cfg!(debug_assertions) {
        return Err("Mock MiniDSP is available only in development builds".into());
    }
    Ok(wanted)
}

/// Runs one ask and returns the status to show, or the error, plus the bass
/// gain to remember when the bass was set.
async fn handle(
    http: &reqwest::Client,
    mock: &mut Option<Mock>,
    config: &MiniDspConfig,
    ask: MiniDspAsk,
) -> (Result<MiniDsp, String>, Option<f64>) {
    if !config.enabled {
        return (Err("MiniDSP screen is disabled".into()), None);
    }
    match mock_wanted(config) {
        Err(e) => return (Err(e), None),
        Ok(true) => {
            let device = mock.get_or_insert_with(|| Mock::new(config));
            return match device.act(config, ask) {
                Ok(bass) => (Ok(device.status()), bass),
                Err(e) => (Err(e), None),
            };
        }
        Ok(false) => *mock = None,
    }
    let client = Client::new(http.clone(), config.clone());
    let done = match ask {
        MiniDspAsk::Refresh => Ok(None),
        MiniDspAsk::SetVolume(v) => client.set_volume(v).await.map(|()| None),
        MiniDspAsk::SetBass(v) => client.set_bass(v).await.map(Some),
        MiniDspAsk::SelectPreset(p) => client.select_preset(p).await.map(|()| None),
    };
    match done {
        Ok(bass) => (client.status().await, bass),
        Err(e) => (Err(e), None),
    }
}

fn shown(path: &str) -> bool {
    path == PAGE || path.starts_with("/minidsp/")
}

async fn serve(core: &Core, http: &reqwest::Client, mock: &mut Option<Mock>, ask: MiniDspAsk) {
    let config = core.hub.config().minidsp;
    if ask != MiniDspAsk::Refresh {
        core.hub.update(|s| s.minidsp.busy = true);
    }
    let (result, bass) = handle(http, mock, &config, ask).await;
    if let Some(gain) = bass
        && let Err(e) = core
            .set_config(&json!({ "minidsp": { "bassGainDb": gain } }))
            .await
    {
        eprintln!("[minidsp] bass gain not saved: {e}");
    }
    core.hub.update(|s| match result {
        Ok(status) => {
            s.minidsp = MiniDsp {
                checked: true,
                ..status
            }
        }
        Err(e) => {
            s.minidsp.checked = true;
            s.minidsp.busy = false;
            s.minidsp.error = Some(e);
        }
    });
}

pub async fn run(core: Arc<Core>, mut asks: mpsc::UnboundedReceiver<MiniDspAsk>) {
    let http = Client::http(TIMEOUT);
    let mut mock = None;
    let mut path = core.ui_path.subscribe();
    let mut poll = tokio::time::interval(POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let ask = tokio::select! {
            Some(ask) = asks.recv() => ask,
            Ok(()) = path.changed() => {
                if !shown(&path.borrow_and_update()) {
                    continue;
                }
                poll.reset();
                MiniDspAsk::Refresh
            }
            _ = poll.tick() => {
                if !shown(&path.borrow()) || !core.hub.config().minidsp.enabled {
                    continue;
                }
                MiniDspAsk::Refresh
            }
        };
        serve(&core, &http, &mut mock, ask).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::Router;
    use axum::body::Bytes;
    use axum::extract::State;
    use axum::http::{Method, StatusCode, Uri};
    use axum::response::IntoResponse;
    use livi_core_proto::config::defaults;

    use super::*;

    type Seen = Arc<Mutex<Vec<(String, String, Value)>>>;
    type Reply = Arc<dyn Fn(&str) -> (StatusCode, String) + Send + Sync>;

    async fn answer(
        State((seen, reply)): State<(Seen, Reply)>,
        method: Method,
        uri: Uri,
        body: Bytes,
    ) -> impl IntoResponse {
        let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let path = uri.path().to_string();
        seen.lock()
            .unwrap()
            .push((method.to_string(), path.clone(), body));
        reply(&path)
    }

    async fn daemon(
        reply: impl Fn(&str) -> (StatusCode, String) + Send + Sync + 'static,
    ) -> (String, Seen) {
        let seen: Seen = Arc::default();
        let reply: Reply = Arc::new(reply);
        let app = Router::new()
            .fallback(answer)
            .with_state((seen.clone(), reply));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        (url, seen)
    }

    fn config(url: &str) -> MiniDspConfig {
        MiniDspConfig {
            server_url: format!("{url}/"),
            ..defaults().minidsp
        }
    }

    fn client(url: &str) -> Client {
        Client::new(Client::http(TIMEOUT), config(url))
    }

    fn ok(body: Value) -> (StatusCode, String) {
        (StatusCode::OK, body.to_string())
    }

    #[test]
    fn the_defaults_match_the_old_page() {
        let c = defaults().minidsp;
        assert!(c.enabled);
        assert_eq!(c.server_url, "http://127.0.0.1:5380");
        assert_eq!((c.volume_min_db, c.volume_max_db), (-80.0, 0.0));
        assert_eq!((c.bass_min_db, c.bass_max_db), (-80.0, 12.0));
        assert_eq!(c.bass_output_channels, vec![2, 3]);
        assert_eq!(c.presets.len(), 4);
        assert_eq!(c.presets[0].label, "Preset 1");
    }

    #[tokio::test]
    async fn no_devices_reads_as_disconnected() {
        let (url, seen) = daemon(|_| ok(json!([]))).await;
        let status = client(&url).status().await.unwrap();
        assert_eq!(
            status,
            MiniDsp {
                volume_db: -80.0,
                ..Default::default()
            }
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            (seen[0].0.as_str(), seen[0].1.as_str()),
            ("GET", "/devices")
        );
    }

    #[tokio::test]
    async fn the_selected_device_status_is_read_after_detection() {
        let (url, seen) = daemon(|path| match path {
            "/devices" => ok(json!([{ "product_name": "2x4HD" }])),
            _ => ok(json!({ "master": { "preset": 2, "source": "Toslink", "volume": -18.5, "mute": false } })),
        })
        .await;
        let status = client(&url).status().await.unwrap();
        assert_eq!(
            status,
            MiniDsp {
                connected: true,
                preset: 2,
                source: "Toslink".into(),
                volume_db: -18.5,
                product_name: Some("2x4HD".into()),
                ..Default::default()
            }
        );
        assert_eq!(seen.lock().unwrap()[1].1, "/devices/0");
    }

    #[tokio::test]
    async fn a_status_without_a_volume_is_invalid() {
        let (url, _) = daemon(|path| match path {
            "/devices" => ok(json!([{}])),
            _ => ok(json!({ "master": {} })),
        })
        .await;
        assert_eq!(
            client(&url).status().await.unwrap_err(),
            "MiniDSP returned an invalid device status"
        );
    }

    #[tokio::test]
    async fn actions_post_clamped_patches_to_the_device() {
        let (url, seen) = daemon(|_| (StatusCode::OK, String::new())).await;
        let mut c = client(&url);
        c.config.device_index = 1;
        c.config.bass_output_channels = vec![3, 2, 3];
        c.set_volume(5.0).await.unwrap();
        assert_eq!(c.set_bass(-100.0).await.unwrap(), -80.0);
        c.select_preset(3).await.unwrap();
        let seen = seen.lock().unwrap();
        let posts: Vec<_> = seen
            .iter()
            .map(|(m, p, b)| (m.as_str(), p.as_str(), b))
            .collect();
        assert_eq!(
            posts,
            vec![
                (
                    "POST",
                    "/devices/1/config",
                    &json!({ "master_status": { "volume": 0.0 } })
                ),
                (
                    "POST",
                    "/devices/1/config",
                    &json!({ "outputs": [{ "index": 2, "gain": -80.0 }, { "index": 3, "gain": -80.0 }] })
                ),
                (
                    "POST",
                    "/devices/1/config",
                    &json!({ "master_status": { "preset": 3 } })
                ),
            ]
        );
    }

    #[tokio::test]
    async fn bad_input_is_refused_before_any_request() {
        let (url, seen) = daemon(|_| ok(json!({}))).await;
        let mut c = client(&url);
        assert_eq!(
            c.select_preset(9).await.unwrap_err(),
            "MiniDSP preset 9 is not configured"
        );
        assert_eq!(
            c.set_volume(f64::NAN).await.unwrap_err(),
            "Volume must be a finite number"
        );
        c.config.bass_output_channels.clear();
        assert_eq!(
            c.set_bass(1.0).await.unwrap_err(),
            "No MiniDSP bass output channels configured"
        );
        c.config.server_url = "ftp://example".into();
        assert_eq!(
            c.status().await.unwrap_err(),
            "MiniDSP server URL must use http or https"
        );
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn daemon_errors_carry_the_status_and_detail() {
        let (url, _) = daemon(|_| (StatusCode::BAD_REQUEST, " no such device \n".into())).await;
        assert_eq!(
            client(&url).set_volume(-10.0).await.unwrap_err(),
            "MiniDSP returned 400: no such device"
        );
        let (url, _) = daemon(|_| (StatusCode::INTERNAL_SERVER_ERROR, String::new())).await;
        assert_eq!(
            client(&url).status().await.unwrap_err(),
            "MiniDSP returned 500"
        );
    }

    #[tokio::test]
    async fn an_unreachable_daemon_says_so() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(
            client(&url).status().await.unwrap_err(),
            format!("Cannot connect to MiniDSP daemon at {url}. Make sure minidspd is running.")
        );
    }

    #[tokio::test]
    async fn a_slow_daemon_times_out() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _held = listener.accept().await;
            std::future::pending::<()>().await;
        });
        let c = Client::new(Client::http(Duration::from_millis(100)), config(&url));
        assert_eq!(
            c.status().await.unwrap_err(),
            "MiniDSP daemon did not respond in time"
        );
    }

    #[tokio::test]
    async fn the_mock_keeps_what_it_was_told() {
        let mut config = defaults().minidsp;
        config.mock_device = true;
        let http = Client::http(TIMEOUT);
        let mut mock = None;
        let (status, _) = handle(&http, &mut mock, &config, MiniDspAsk::SetVolume(-200.0)).await;
        let status = status.unwrap();
        assert_eq!(status.volume_db, -80.0);
        assert_eq!(status.product_name.as_deref(), Some("Mock MiniDSP"));
        let (status, _) = handle(&http, &mut mock, &config, MiniDspAsk::SelectPreset(2)).await;
        assert_eq!(status.unwrap().preset, 2);
        let (_, bass) = handle(&http, &mut mock, &config, MiniDspAsk::SetBass(40.0)).await;
        assert_eq!(bass, Some(12.0));
        let (status, _) = handle(&http, &mut mock, &config, MiniDspAsk::SelectPreset(7)).await;
        assert_eq!(status.unwrap_err(), "MiniDSP preset 7 is not configured");

        config.enabled = false;
        let (status, _) = handle(&http, &mut mock, &config, MiniDspAsk::Refresh).await;
        assert_eq!(status.unwrap_err(), "MiniDSP screen is disabled");
    }

    #[test]
    fn only_the_minidsp_page_polls() {
        assert!(shown("/minidsp"));
        assert!(!shown("/media"));
        assert!(!shown("/minidspx"));
    }
}
