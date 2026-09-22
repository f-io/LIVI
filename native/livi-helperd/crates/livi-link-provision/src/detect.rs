use crate::shell::{self, Shell};
use crate::v821b;

pub enum Detected {
    Cpc200 { host: String },
    LiviLink { model: String },
    /// A VehiConn V821B in stock firmware, reachable through its own WiFi AP.
    V821bStock { info: v821b::web::HostInfo },
    Nothing,
}

impl Detected {
    pub fn label(&self) -> String {
        match self {
            Detected::Cpc200 { host } => format!("CPC200-CCPA at {host}"),
            Detected::LiviLink { model } => format!("{model} already running LIVI Link"),
            Detected::V821bStock { info } => format!(
                "V821B+AIC8800D80 in stock firmware ({}, appver {})",
                info.name, info.sys.appver
            ),
            Detected::Nothing => "no dongle found".into(),
        }
    }
}

pub fn detect() -> Detected {
    if let Some(model) = livi_model(shell::DEFAULT_HOST) {
        return Detected::LiviLink { model };
    }
    if let Ok(info) = v821b::web::host() {
        return Detected::V821bStock { info };
    }
    for host in [shell::DEFAULT_HOST, "192.168.50.2"] {
        if Shell::new(host).port_open(shell::TELNET_PORT) {
            return Detected::Cpc200 {
                host: host.to_string(),
            };
        }
    }
    Detected::Nothing
}

/// The model a LIVI-Link dongle reports on its web API
fn livi_model(host: &str) -> Option<String> {
    let url = format!("http://{host}/api/status");
    let resp = ureq::get(&url).timeout(std::time::Duration::from_secs(2)).call().ok()?;
    let json: serde_json::Value = resp.into_json().ok()?;
    json.get("model")?.as_str().map(str::to_string)
}
