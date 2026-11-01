// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::dongle;
use crate::dongle::arm::imx6ul::shell::{self, Shell};

pub enum Detected {
    Imx6ul { host: String },
    Rescue { family: dongle::probe::Family },
    LiviLink { model: String, target: String },
    DongleStock { info: dongle::web::HostInfo },
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
