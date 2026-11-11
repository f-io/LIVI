// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;

use crate::system::privileged::{Privileged, sudo, sudo_grants, username};

const GUARD: &str = "/usr/local/lib/livi/gvfs-phone-guard.sh";
const MARKER: &str = "gvfs-guard-v1.installed";
const MONITOR_DIR: &str = "/usr/share/gvfs/remote-volume-monitors";
const PHONE_MONITORS: [&str; 3] = ["afc", "gphoto2", "mtp"];

const SCRIPT: &str = r#"#!/bin/bash
set -u
D=/usr/share/gvfs/remote-volume-monitors
action="${1:-}"
for m in afc gphoto2 mtp; do
  case "$action" in
    disable) [ -f "$D/$m.monitor" ] && mv "$D/$m.monitor" "$D/$m.livi-off" ;;
    restore) [ -f "$D/$m.livi-off" ] && mv "$D/$m.livi-off" "$D/$m.monitor" ;;
    *) echo "usage: gvfs-phone-guard.sh disable|restore" >&2 ; exit 2 ;;
  esac
done
[ "$action" = disable ] && pkill -f "gvfs-afc-volume|gvfs-gphoto2|gvfs-mtp-volume|gvfsd-afc" 2>/dev/null
exit 0
"#;

fn rule(user: &str) -> String {
    format!(
        "# Installed by LIVI — lets {user} toggle the phone gvfs volume monitors while LIVI\n\
         # runs, and restore them when it exits. Remove this file to revoke.\n\
         Cmnd_Alias LIVI_GVFS = {GUARD} disable, {GUARD} restore\n\
         {user} ALL=(root) NOPASSWD: LIVI_GVFS\n"
    )
}

fn monitors_present() -> bool {
    PHONE_MONITORS.iter().any(|m| {
        Path::new(&format!("{MONITOR_DIR}/{m}.monitor")).exists()
            || Path::new(&format!("{MONITOR_DIR}/{m}.livi-off")).exists()
    })
}

async fn installed(p: &Privileged, rule: &str) -> bool {
    Path::new(GUARD).exists() && (sudo_grants(GUARD).await || p.marker_holds(MARKER, rule))
}

pub async fn start(p: Privileged) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let rule = rule(&username());
    if !installed(&p, &rule).await && !crate::system::power::owns_host() && monitors_present() {
        if p.helper_installs("install-gvfs-guard", &[("script", SCRIPT), ("rule", &rule)])
            .await
        {
            p.write_marker(MARKER, &rule);
        } else {
            eprintln!("[gvfs] the phone guard is not installed, run the LIVI install script");
        }
    }
    if !Path::new(GUARD).exists() {
        return;
    }
    if !(sudo(&[GUARD, "restore"]).await && sudo(&[GUARD, "disable"]).await) {
        eprintln!("[gvfs] the phone monitors stay, the guard did not run");
        return;
    }
    let watch = format!(
        "while kill -0 {} 2>/dev/null; do sleep 2; done; sudo -n {GUARD} restore",
        std::process::id()
    );
    let spawned = std::process::Command::new("bash")
        .args(["-c", &watch])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    if let Err(e) = spawned {
        eprintln!("[gvfs] no watcher, the monitors come back only on a clean quit: {e}");
    }
}

pub async fn stop() {
    if cfg!(target_os = "linux") && Path::new(GUARD).exists() {
        sudo(&[GUARD, "restore"]).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_lets_only_the_guard_run() {
        let r = rule("u");
        assert!(r.contains(&format!(
            "Cmnd_Alias LIVI_GVFS = {GUARD} disable, {GUARD} restore"
        )));
        assert!(r.ends_with("u ALL=(root) NOPASSWD: LIVI_GVFS\n"));
        assert!(SCRIPT.starts_with("#!/bin/bash\n"));
    }
}
