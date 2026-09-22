use crate::dongle;
use crate::dongle::arm::imx6ul::shell::{self, Shell};

pub enum Detected {
    Imx6ul { host: String },
    LiviLink { model: String },
    /// A dongle in stock firmware, running the "Liaoyuan" web/OTA stack (`dongle::web`) — could
    /// be a V821B or an AX520 (or another project not yet seen), see `dongle::hook::ly_project`.
    DongleStock { info: dongle::web::HostInfo },
    Nothing,
}

impl Detected {
    pub fn label(&self) -> String {
        match self {
            Detected::Imx6ul { host } => format!("CPC200-CCPA at {host}"),
            Detected::LiviLink { model } => format!("{model} already running LIVI Link"),
            Detected::DongleStock { info } => {
                let project = dongle::hook::ly_project(&info.sys.appver)
                    .unwrap_or_else(|| "unknown project".into());
                format!("{project} dongle in stock firmware ({}, appver {})", info.name, info.sys.appver)
            }
            Detected::Nothing => "no dongle found".into(),
        }
    }
}

pub fn detect() -> Detected {
    if let Some(model) = livi_model(shell::DEFAULT_HOST) {
        return Detected::LiviLink { model };
    }
    if let Ok(info) = dongle::web::host() {
        return Detected::DongleStock { info };
    }
    for host in [shell::DEFAULT_HOST, "192.168.50.2"] {
        if Shell::new(host).port_open(shell::TELNET_PORT) {
            return Detected::Imx6ul {
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
