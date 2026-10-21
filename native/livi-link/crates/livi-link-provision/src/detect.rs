use crate::dongle;
use crate::dongle::arm::imx6ul::shell::{self, Shell};

pub enum Detected {
    Imx6ul {
        host: String,
    },
    Rescue {
        family: dongle::probe::Family,
    },
    LiviLink {
        model: String,
        target: String,
    },
    DongleStock {
        info: dongle::web::HostInfo,
    },
    X1600Stock {
        version: dongle::mips::x1600::Version,
    },
    Nothing,
}

impl Detected {
    pub fn label(&self) -> String {
        match self {
            Detected::Imx6ul { host } => format!("i.MX6UL dongle with a shell at {host}"),
            Detected::Rescue { family } => {
                format!(
                    "{} in its rescue system, without a LIVI Link rootfs",
                    family.name()
                )
            }
            Detected::LiviLink { model, .. } => format!("{model} already running LIVI Link"),
            Detected::DongleStock { info } => {
                let project = dongle::hook::ly_project(&info.sys.appver)
                    .unwrap_or_else(|| "unknown project".into());
                format!(
                    "{project} dongle in stock firmware ({}, appver {})",
                    info.name, info.sys.appver
                )
            }
            Detected::X1600Stock { version } => format!(
                "Ingenic X1600 dongle in stock firmware ({}, platform {}, system {})",
                version.model,
                version.platform.as_deref().unwrap_or("?"),
                version.system_version.as_deref().unwrap_or("?")
            ),
            Detected::Nothing => "no dongle found".into(),
        }
    }
}

pub fn detect() -> Detected {
    if let Some(status) = dongle::link::status(shell::DEFAULT_HOST) {
        return Detected::LiviLink {
            model: status.model,
            target: status.target,
        };
    }
    if let Ok(info) = dongle::web::host() {
        return Detected::DongleStock { info };
    }
    if let Some(version) = dongle::mips::x1600::detect(dongle::DONGLE_HOST) {
        return Detected::X1600Stock { version };
    }
    let sh = Shell::new(shell::DEFAULT_HOST);
    if sh.port_open(shell::TELNET_PORT) {
        if let Some(family) = dongle::rescue::of(&sh) {
            return Detected::Rescue { family };
        }
        return Detected::Imx6ul {
            host: shell::DEFAULT_HOST.to_string(),
        };
    }
    Detected::Nothing
}
