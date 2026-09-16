use crate::shell::{self, Shell};
use crate::v821b;

pub enum Detected {
    /// A CPC200-CCPA already running our root shell, or the stock one at the legacy IP.
    Cpc200 { host: String },
    /// A VehiConn V821B in stock firmware, reachable through its own WiFi AP.
    V821bStock { info: v821b::web::HostInfo },
    /// A V821B running LIVI Link firmware — our own httpd on 10.10.10.1.
    V821bLink,
    Nothing,
}

impl Detected {
    pub fn label(&self) -> String {
        match self {
            Detected::Cpc200 { host } => format!("CPC200-CCPA at {host}"),
            Detected::V821bStock { info } => format!(
                "V821B+AIC8800D80 in stock firmware ({}, appver {})",
                info.name, info.sys.appver
            ),
            Detected::V821bLink => "V821B+AIC8800D80 already running LIVI Link".into(),
            Detected::Nothing => "no dongle found".into(),
        }
    }
}

pub fn detect() -> Detected {
    if let Ok(info) = v821b::web::host() {
        return Detected::V821bStock { info };
    }
    if Shell::new("10.10.10.1").port_open(80) {
        return Detected::V821bLink;
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
