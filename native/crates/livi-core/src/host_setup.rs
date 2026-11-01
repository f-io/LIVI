// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};

const BT_ROLES: &str = "monitor.bluez.properties = {
  bluez5.roles = [ a2dp_sink a2dp_source ]
}
";

fn bt_roles_file(home: &Path) -> PathBuf {
    home.join(".config/wireplumber/wireplumber.conf.d/99-livi-hfp.conf")
}

fn write_bt_roles(file: &Path) -> std::io::Result<bool> {
    if std::fs::read_to_string(file).is_ok_and(|current| current == BT_ROLES) {
        return Ok(false);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, BT_ROLES)?;
    Ok(true)
}

pub fn prepare() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let file = bt_roles_file(&home);
    match write_bt_roles(&file) {
        Ok(false) => {}
        Ok(true) => {
            println!("[setup] {} written, restarting wireplumber", file.display());
            tokio::spawn(async {
                let restart = tokio::process::Command::new("systemctl")
                    .args(["--user", "restart", "wireplumber"])
                    .status()
                    .await;
                if !restart.is_ok_and(|s| s.success()) {
                    eprintln!("[setup] wireplumber did not restart");
                }
            });
        }
        Err(e) => eprintln!("[setup] {} not written: {e}", file.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;

    #[test]
    fn the_roles_are_written_once() {
        let dir = TempDir::new();
        let file = bt_roles_file(&dir.0);
        assert!(write_bt_roles(&file).unwrap());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), BT_ROLES);
        assert!(!write_bt_roles(&file).unwrap());
        std::fs::write(&file, "old").unwrap();
        assert!(write_bt_roles(&file).unwrap());
    }
}
