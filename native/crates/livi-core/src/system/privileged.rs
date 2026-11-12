// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

const SUDO_TIMEOUT: Duration = Duration::from_secs(12);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct Privileged {
    pub templates: PathBuf,
    pub helper: PathBuf,
    pub helper_named: PathBuf,
    pub user_data: PathBuf,
}

pub fn username() -> String {
    if let Ok(name) = std::env::var("SUDO_USER")
        && !name.is_empty()
    {
        return name;
    }
    // SAFETY: getpwuid returns a pointer into static storage or null, read at once.
    let entry = unsafe { libc::getpwuid(libc::geteuid()) };
    if !entry.is_null() {
        // SAFETY: pw_name of a valid entry is a nul-terminated string.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).pw_name) };
        return name.to_string_lossy().into_owned();
    }
    std::env::var("USER").unwrap_or_default()
}

pub fn systemctl() -> String {
    [
        "/usr/bin/systemctl",
        "/bin/systemctl",
        "/usr/sbin/systemctl",
    ]
    .into_iter()
    .find(|p| Path::new(p).exists())
    .unwrap_or("/usr/bin/systemctl")
    .to_string()
}

fn stamp(content: &str) -> String {
    livi_base::hex::encode(&livi_crypto::sha256(&[content.as_bytes()])[..8])
}

pub async fn sudo(args: &[&str]) -> bool {
    sudo_within(args, SUDO_TIMEOUT).await
}

pub async fn sudo_within(args: &[&str], limit: Duration) -> bool {
    let mut sudo_args = vec!["-n"];
    sudo_args.extend_from_slice(args);
    quietly_within("sudo", &sudo_args, limit).await
}

pub async fn sudo_grants(needle: &str) -> bool {
    let Ok(out) = Command::new("sudo")
        .args(["-n", "-l"])
        .stdin(Stdio::null())
        .output()
        .await
    else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    out.status.success() && (text.contains(needle) || grants_all(&text))
}

fn grants_all(sudo_list: &str) -> bool {
    sudo_list.lines().any(|line| {
        let line = line.trim();
        (line.starts_with("(ALL) NOPASSWD:") || line.starts_with("(ALL : ALL) NOPASSWD:"))
            && line.ends_with(" ALL")
    })
}

pub async fn quietly(program: &str, args: &[&str]) -> bool {
    quietly_within(program, args, SUDO_TIMEOUT).await
}

async fn quietly_within(program: &str, args: &[&str], limit: Duration) -> bool {
    let run = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .status();
    matches!(tokio::time::timeout(limit, run).await, Ok(Ok(s)) if s.success())
}

impl Privileged {
    pub fn render(&self, name: &str) -> std::io::Result<String> {
        let text = std::fs::read_to_string(self.templates.join(name))?;
        Ok(text
            .replace("__HELPER__", &self.helper_named.display().to_string())
            .replace("__USERNAME__", &username())
            .replace("__USER_DATA__", &self.user_data.display().to_string())
            .replace("__SYSTEMCTL__", &systemctl()))
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.user_data.join(name)
    }

    pub fn marker_holds(&self, name: &str, content: &str) -> bool {
        std::fs::read_to_string(self.marker(name)).is_ok_and(|m| m.trim() == stamp(content))
    }

    pub fn write_marker(&self, name: &str, content: &str) {
        if let Err(e) = std::fs::write(self.marker(name), stamp(content)) {
            eprintln!("[setup] {name} not noted: {e}");
        }
    }

    pub async fn helper_installs(&self, what: &str, files: &[(&str, &str)]) -> bool {
        let dir = std::env::temp_dir().join(format!("livi-install-{}-{what}", std::process::id()));
        let staged: std::io::Result<Vec<String>> = (|| {
            std::fs::create_dir_all(&dir)?;
            files
                .iter()
                .map(|(name, content)| {
                    let path = dir.join(name);
                    std::fs::write(&path, content)?;
                    Ok(path.display().to_string())
                })
                .collect()
        })();
        let ok = match staged {
            Ok(paths) => {
                let switch = format!("--{what}");
                let helper = self.helper.display().to_string();
                let mut args = vec!["-n", helper.as_str(), switch.as_str()];
                args.extend(paths.iter().map(String::as_str));
                let run = Command::new("sudo")
                    .args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .status();
                matches!(tokio::time::timeout(INSTALL_TIMEOUT, run).await, Ok(Ok(s)) if s.success())
            }
            Err(e) => {
                eprintln!("[setup] {what} not staged: {e}");
                false
            }
        };
        let _ = std::fs::remove_dir_all(&dir);
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::config_file::tests::TempDir;

    fn privileged(dir: &TempDir) -> Privileged {
        Privileged {
            templates: dir.0.clone(),
            helper: dir.0.join("helper"),
            helper_named: PathBuf::from("/home/u/.config/LIVI/driver/livi-helperd"),
            user_data: dir.0.clone(),
        }
    }

    #[test]
    fn a_template_gets_this_machine_and_a_marker_its_stamp() {
        let dir = TempDir::new();
        std::fs::write(
            dir.0.join("t"),
            "__HELPER__ for __USERNAME__ via __SYSTEMCTL__ in __USER_DATA__",
        )
        .unwrap();
        let p = privileged(&dir);
        let text = p.render("t").unwrap();
        assert!(text.starts_with("/home/u/.config/LIVI/driver/livi-helperd for "));
        assert!(!text.contains("__"));
        assert!(!p.marker_holds(".m", "a"));
        p.write_marker(".m", "a");
        assert!(p.marker_holds(".m", "a"));
        assert!(!p.marker_holds(".m", "b"));
        assert_eq!(stamp("a").len(), 16);
    }

    #[test]
    fn only_a_rule_for_everything_grants_everything() {
        assert!(grants_all(
            "User u may run:\n    (ALL : ALL) NOPASSWD: ALL\n"
        ));
        assert!(grants_all("    (ALL) NOPASSWD: ALL"));
        assert!(!grants_all(
            "    (root) NOPASSWD: /usr/bin/systemctl restart x"
        ));
        assert!(!grants_all("    (ALL : ALL) ALL"));
    }
}
