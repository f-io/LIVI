// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use crate::system::privileged::Privileged;

const UNIT_FILE: &str = "/etc/systemd/system/livi-kiosk.service";
const TEMPLATE: &str = "livi-kiosk.service.template";

fn version_line(template: &str) -> Option<&str> {
    template
        .lines()
        .find(|l| l.starts_with("# LIVI-KIOSK-VERSION="))
}

fn appimage_of(unit: &str) -> Option<String> {
    let exec = unit
        .lines()
        .find_map(|l| l.trim().strip_prefix("ExecStart="))?;
    let path = exec
        .rsplit_once(" -- ")
        .map_or(exec, |(_, after)| after)
        .trim();
    (path.ends_with(".AppImage") && !path.contains(' ')).then(|| path.to_string())
}

fn render(template: &str, home: &str, appimage: &str) -> String {
    template
        .replace("__HOME__", home)
        .replace("__APPIMAGE__", appimage)
}

pub async fn ensure(p: &Privileged) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let Ok(unit) = std::fs::read_to_string(UNIT_FILE) else {
        return false;
    };
    let template = match p.render(TEMPLATE) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[kiosk] {TEMPLATE} does not read: {e}");
            return false;
        }
    };
    let Some(version) = version_line(&template) else {
        return false;
    };
    if unit.contains(version) {
        return false;
    }
    let appimage = std::env::var("APPIMAGE")
        .ok()
        .filter(|p| Path::new(p).exists())
        .or_else(|| appimage_of(&unit));
    let (Some(appimage), Ok(home)) = (appimage, std::env::var("HOME")) else {
        eprintln!("[kiosk] {UNIT_FILE} left as it is, the AppImage path is unknown");
        return false;
    };
    let content = render(&template, &home, &appimage);
    if !p
        .helper_installs("install-kiosk-unit", &[("unit", &content)])
        .await
    {
        eprintln!("[kiosk] {UNIT_FILE} not updated, run the LIVI install script");
        return false;
    }
    println!("[kiosk] {UNIT_FILE} updated, the kiosk starts again without cage");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_UNIT: &str = "[Service]\nUser=pi\n\
        ExecStart=/usr/bin/cage -s -- /home/pi/LIVI/LIVI.AppImage\nRestart=no\n";

    #[test]
    fn an_old_unit_names_its_appimage_behind_cage() {
        assert_eq!(
            appimage_of(OLD_UNIT).as_deref(),
            Some("/home/pi/LIVI/LIVI.AppImage")
        );
        let direct = "ExecStart=/home/pi/LIVI/LIVI.AppImage\n";
        assert_eq!(
            appimage_of(direct).as_deref(),
            Some("/home/pi/LIVI/LIVI.AppImage")
        );
        assert_eq!(appimage_of("ExecStart=/usr/bin/cage -s -- /opt/x\n"), None);
        assert_eq!(appimage_of("[Service]\n"), None);
    }

    #[test]
    fn the_template_brings_the_version_and_this_machine() {
        let template =
            "# LIVI-KIOSK-VERSION=2\nWorkingDirectory=__HOME__\nExecStart=__APPIMAGE__\n";
        assert_eq!(version_line(template), Some("# LIVI-KIOSK-VERSION=2"));
        assert!(!OLD_UNIT.contains(version_line(template).unwrap()));
        let unit = render(template, "/home/pi", "/home/pi/LIVI/LIVI.AppImage");
        assert_eq!(
            unit,
            "# LIVI-KIOSK-VERSION=2\nWorkingDirectory=/home/pi\nExecStart=/home/pi/LIVI/LIVI.AppImage\n"
        );
        assert_eq!(version_line("ExecStart=x\n"), None);
    }
}
