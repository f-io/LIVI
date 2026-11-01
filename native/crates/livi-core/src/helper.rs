// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use std::{fs, io};

use livi_core_proto::config::{CarType, Config};
use livi_cp::identity::Identity;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, watch};

const HELPER_BIN: &str = "livi-helperd";
const STALE_PATTERN: &str = "driver/livi-helperd$";
const STOP_GRACE: Duration = Duration::from_secs(3);

pub fn debug(cfg: &Config) -> bool {
    cfg.debug_logging || std::env::var("DEBUG").is_ok_and(|v| v == "1")
}

pub fn helper_env(
    cfg: &Config,
    identity: &Identity,
    accessory: &str,
    airplay_port: Option<u16>,
) -> Vec<(String, String)> {
    let mut env = settings_env(cfg);
    env.push(("LIVI_CP_DEVICE_ID".to_string(), accessory.to_string()));
    env.push(("LIVI_CP_PK".to_string(), identity.pk_hex()));
    env.push(("LIVI_CP_PI".to_string(), identity.pairing_id.clone()));
    if let Some(port) = airplay_port.filter(|p| *p > 0) {
        env.push(("LIVI_CP_AIRPLAY_PORT".to_string(), port.to_string()));
    }
    env
}

pub fn starts_differently(before: &Config, after: &Config) -> bool {
    settings_env(before) != settings_env(after)
}

fn cp_fuels(car: Option<CarType>) -> &'static str {
    match car {
        Some(CarType::Gasoline | CarType::E85) => "Gasoline",
        Some(CarType::Diesel | CarType::DieselWinter | CarType::Biodiesel) => "Diesel",
        Some(CarType::Electric) => "Electric",
        Some(CarType::Cng) => "CNG",
        Some(CarType::HybridGasoline) => "Gasoline,Electric",
        Some(CarType::HybridDiesel) => "Diesel,Electric",
        _ => "",
    }
}

fn cp_connectors(cfg: &Config) -> String {
    use livi_core_proto::config::EvConnectorType as T;
    let watts = |kw: Option<f64>| kw.map(|kw| format!(":{}", (kw * 1000.0).round() as u64));
    cfg.ev_connector_types
        .iter()
        .flatten()
        .filter_map(|plug| {
            let (name, kw) = match plug {
                T::J1772 => ("J1772", cfg.ev_max_ac_power_kw),
                T::Mennekes => ("Mennekes", cfg.ev_max_ac_power_kw),
                T::Chademo => ("Chademo", cfg.ev_max_dc_power_kw),
                T::Combo1 => ("Combo1", cfg.ev_max_dc_power_kw),
                T::Combo2 => ("Combo2", cfg.ev_max_dc_power_kw),
                T::TeslaSupercharger => ("TeslaSupercharger", cfg.ev_max_dc_power_kw),
                T::Gbt => ("Gbt", cfg.ev_max_dc_power_kw),
                T::Unknown | T::Other => return None,
            };
            Some(format!("{name}{}", watts(kw).unwrap_or_default()))
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn settings_env(cfg: &Config) -> Vec<(String, String)> {
    let flag = |on: bool, off: &str| if on { "1".to_string() } else { off.to_string() };
    let debug = debug(cfg);
    let car_name = if cfg.car_name.is_empty() {
        std::env::var("LIVI_CP_NAME").unwrap_or_default()
    } else {
        cfg.car_name.clone()
    };
    let channel = if cfg.wifi_channel == 0 {
        String::new()
    } else {
        cfg.wifi_channel.to_string()
    };
    vec![
        (
            "LIVI_AA_WIRELESS".to_string(),
            flag(cfg.wireless_aa_enabled, "0"),
        ),
        (
            "LIVI_CP_WIRELESS".to_string(),
            flag(cfg.wireless_cp_enabled, ""),
        ),
        ("DEBUG".to_string(), flag(debug, "")),
        ("LIVI_CP_NAME".to_string(), car_name),
        (
            "LIVI_CP_FUELS".to_string(),
            cp_fuels(cfg.car_type).to_string(),
        ),
        ("LIVI_CP_CONNECTORS".to_string(), cp_connectors(cfg)),
        ("LIVI_CP_THEME".to_string(), cfg.car_play_theme.clone()),
        ("LIVI_BT_ADAPTER".to_string(), cfg.bt_adapter.clone()),
        ("LIVI_MFI_FILES".to_string(), cfg.car_play_mfi_files.clone()),
        ("LIVI_WIFI_IFACE".to_string(), cfg.wifi_interface.clone()),
        ("LIVI_PASSPHRASE".to_string(), cfg.wifi_password.clone()),
        (
            "LIVI_WIFI_TYPE".to_string(),
            livi_wifi::Band::setting(crate::wifi_options::band_of(cfg.wifi_type)).to_string(),
        ),
        ("LIVI_CHANNEL".to_string(), channel),
        ("LIVI_COUNTRY".to_string(), cfg.country.clone()),
        ("LIVI_CP_DEBUG".to_string(), flag(debug, "")),
    ]
}

fn inside_appimage_mount(p: &Path) -> bool {
    let appdir = std::env::var_os("APPDIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    (std::env::var_os("APPIMAGE").is_some() && p.starts_with(&appdir))
        || p.to_string_lossy().contains("/.mount_")
}

fn digest(p: &Path) -> io::Result<Vec<u8>> {
    Ok(Sha256::digest(fs::read(p)?).to_vec())
}

fn stage(src: &Path, dest: &Path) -> io::Result<()> {
    if dest.exists() && digest(dest)? == digest(src)? {
        return Ok(());
    }
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = dest.with_extension("new");
    fs::copy(src, &tmp)?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    fs::rename(&tmp, dest)
}

pub fn resolve_bin(bin: &Path, user_data: &Path) -> PathBuf {
    if !cfg!(target_os = "linux") || !bin.exists() || !inside_appimage_mount(bin) {
        return bin.to_path_buf();
    }
    let staged = user_data.join("driver").join(HELPER_BIN);
    match stage(bin, &staged) {
        Ok(()) => staged,
        Err(e) => {
            eprintln!("[core] staging the helper failed: {e}");
            if staged.exists() {
                staged
            } else {
                bin.to_path_buf()
            }
        }
    }
}

/// The helper to run: under LIVI OS the image's own, never a staged copy.
pub fn locate(bin: &Path, user_data: &Path) -> PathBuf {
    if crate::livi_os::active() {
        crate::livi_os::helper()
    } else {
        resolve_bin(bin, user_data)
    }
}

fn stale_pattern(spec: &Spec) -> String {
    if spec.livi_os {
        format!("{}$", spec.bin.display())
    } else {
        STALE_PATTERN.to_string()
    }
}

fn stale_running(pattern: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .is_ok_and(|o| !o.stdout.is_empty())
}

async fn kill_stale(pattern: &str) {
    if !stale_running(pattern) {
        return;
    }
    println!("[core] stopping a stale helper");
    for signal in ["-TERM", "-KILL"] {
        let _ = std::process::Command::new("sudo")
            .args(["-n", "pkill", signal, "-f", pattern])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if !stale_running(pattern) {
                return;
            }
        }
    }
    eprintln!("[core] a stale helper outlived SIGKILL");
}

pub struct Spec {
    pub bin: PathBuf,
    pub env: Box<dyn Fn() -> Vec<(String, String)> + Send + Sync>,
    pub restart: Arc<Notify>,
    pub use_sudo: bool,
    /// LIVI OS: `bin` is the image's fixed helper, run without `-E` and with only the
    /// allow-listed environment of `livi_os::helper_env`.
    pub livi_os: bool,
    pub restart_delay: Duration,
    pub max_restarts: Option<u32>,
}

pub struct HelperSupervisor {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl HelperSupervisor {
    pub fn start(spec: Spec) -> Self {
        let (stop, stop_rx) = watch::channel(false);
        Self {
            stop,
            task: tokio::spawn(run(spec, stop_rx)),
        }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

fn command(spec: &Spec) -> Command {
    let mut cmd = if spec.livi_os {
        let mut c = Command::new("sudo");
        c.arg("-n").arg(&spec.bin).env_clear();
        c
    } else if spec.use_sudo {
        let mut c = Command::new("sudo");
        c.arg("-n").arg("-E").arg(&spec.bin);
        c
    } else {
        Command::new(&spec.bin)
    };
    if let Some(dir) = spec.bin.parent() {
        cmd.current_dir(dir);
    }
    let env = if spec.livi_os {
        crate::livi_os::helper_env((spec.env)(), |k| std::env::var(k).ok())
    } else {
        (spec.env)()
    };
    crate::child::with_lifeline(&mut cmd).envs(env);
    cmd
}

fn spawn(spec: &Spec) -> io::Result<Child> {
    command(spec)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
}

fn forward_lines(child: &mut Child) {
    if let Some(out) = child.stdout.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    println!("[helper] {line}");
                }
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    eprintln!("[helper!] {line}");
                }
            }
        });
    }
}

async fn run(spec: Spec, mut stop: watch::Receiver<bool>) {
    let mut restarts = 0;
    loop {
        if cfg!(target_os = "linux") && (spec.use_sudo || spec.livi_os) {
            kill_stale(&stale_pattern(&spec)).await;
        }
        if !spec.bin.exists() {
            eprintln!("[core] {HELPER_BIN} not found at {}", spec.bin.display());
            return;
        }
        let mut child = match spawn(&spec) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[core] cannot start the helper: {e}");
                return;
            }
        };
        let _lifeline = crate::child::hold_lifeline(&mut child);
        forward_lines(&mut child);
        tokio::select! {
            status = child.wait() => println!("[core] helper exited: {status:?}"),
            _ = spec.restart.notified() => {
                println!("[core] restarting the helper for new settings");
                crate::child::terminate(&mut child, STOP_GRACE, false).await;
                restarts = 0;
                continue;
            }
            _ = stop.changed() => {
                crate::child::terminate(&mut child, STOP_GRACE, false).await;
                return;
            }
        }

        restarts += 1;
        if spec.max_restarts.is_some_and(|max| restarts > max) {
            eprintln!("[core] {HELPER_BIN} exceeded {restarts} restarts, giving up");
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep(spec.restart_delay) => {}
            _ = stop.changed() => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use livi_core_proto::config::WifiBand;

    use super::*;
    use crate::config_file::defaults;
    use crate::config_file::tests::TempDir;

    fn identity() -> Identity {
        Identity {
            secret: [1; 32],
            public: [0xab; 32],
            pairing_id: "pi-1".into(),
        }
    }

    fn get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn env_carries_the_config_and_identity() {
        let mut cfg = defaults();
        cfg.wireless_aa_enabled = true;
        cfg.wifi_type = WifiBand::Ghz24;
        let env = helper_env(&cfg, &identity(), "88:00:33:77:8C:26", Some(7000));
        assert_eq!(get(&env, "LIVI_AA_WIRELESS"), Some("1"));
        assert_eq!(get(&env, "LIVI_CP_WIRELESS"), Some(""));
        assert_eq!(get(&env, "LIVI_CP_PK"), Some("ab".repeat(32).as_str()));
        assert_eq!(get(&env, "LIVI_CP_PI"), Some("pi-1"));
        assert_eq!(get(&env, "LIVI_CP_NAME"), Some("LIVI"));
        assert_eq!(get(&env, "LIVI_CP_FUELS"), Some("Gasoline"));
        assert_eq!(get(&env, "LIVI_CP_CONNECTORS"), Some(""));
        assert_eq!(get(&env, "LIVI_CP_THEME"), Some(""));
        assert_eq!(get(&env, "LIVI_CHANNEL"), Some("36"));
        assert_eq!(get(&env, "LIVI_WIFI_TYPE"), Some("2.4ghz"));
        assert_eq!(get(&env, "LIVI_CP_AIRPLAY_PORT"), Some("7000"));
        cfg.ev_connector_types = Some(vec![
            livi_core_proto::config::EvConnectorType::Combo2,
            livi_core_proto::config::EvConnectorType::Mennekes,
            livi_core_proto::config::EvConnectorType::Chademo,
            livi_core_proto::config::EvConnectorType::Other,
        ]);
        cfg.ev_max_dc_power_kw = Some(150.0);
        cfg.ev_max_ac_power_kw = Some(11.0);
        let env = helper_env(&cfg, &identity(), "88:00:33:77:8C:26", None);
        assert_eq!(
            get(&env, "LIVI_CP_CONNECTORS"),
            Some("Combo2:150000,Mennekes:11000,Chademo:150000")
        );
        cfg.ev_max_dc_power_kw = None;
        let env = helper_env(&cfg, &identity(), "88:00:33:77:8C:26", None);
        assert_eq!(
            get(&env, "LIVI_CP_CONNECTORS"),
            Some("Combo2,Mennekes:11000,Chademo")
        );
        cfg.car_play_theme = "44C9FA31EC:985:19.0".into();
        let env = helper_env(&cfg, &identity(), "88:00:33:77:8C:26", None);
        assert_eq!(get(&env, "LIVI_CP_THEME"), Some("44C9FA31EC:985:19.0"));
        cfg.debug_logging = true;
        let env = helper_env(&cfg, &identity(), "88:00:33:77:8C:26", None);
        assert_eq!(get(&env, "LIVI_CP_AIRPLAY_PORT"), None);
        assert_eq!(get(&env, "DEBUG"), Some("1"));
    }

    fn spec(livi_os: bool, use_sudo: bool) -> Spec {
        Spec {
            bin: PathBuf::from(if livi_os {
                crate::livi_os::DEFAULT_HELPER
            } else {
                "/home/u/.config/LIVI/driver/livi-helperd"
            }),
            env: Box::new(|| {
                vec![
                    ("LIVI_CP_PK".into(), "ab".into()),
                    ("NOT_FOR_THE_HELPER".into(), "x".into()),
                ]
            }),
            restart: Arc::new(Notify::new()),
            use_sudo,
            livi_os,
            restart_delay: Duration::from_secs(1),
            max_restarts: None,
        }
    }

    fn line(cmd: &Command) -> Vec<String> {
        let cmd = cmd.as_std();
        std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn envs(cmd: &Command) -> Vec<(String, Option<String>)> {
        cmd.as_std()
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn livi_os_runs_the_fixed_helper_without_the_callers_environment() {
        let cmd = command(&spec(true, true));
        assert_eq!(line(&cmd), ["sudo", "-n", "/usr/libexec/livi-helperd"]);
        let env = envs(&cmd);
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"PATH"));
        assert!(keys.contains(&"LIVI_CP_PK"));
        assert!(keys.contains(&livi_core_proto::LIFELINE_ENV));
        assert!(!keys.contains(&"NOT_FOR_THE_HELPER"));
        assert!(keys.iter().all(|k| *k == "PATH"
            || crate::livi_os::HELPER_KEYS.contains(k)
            || crate::livi_os::PASSTHROUGH_KEYS.contains(k)));
        assert_eq!(
            stale_pattern(&spec(true, true)),
            "/usr/libexec/livi-helperd$"
        );
    }

    #[test]
    fn off_livi_os_the_helper_keeps_sudo_dash_e() {
        let cmd = command(&spec(false, true));
        assert_eq!(
            line(&cmd),
            [
                "sudo",
                "-n",
                "-E",
                "/home/u/.config/LIVI/driver/livi-helperd"
            ]
        );
        assert!(
            envs(&cmd)
                .iter()
                .any(|(k, v)| k == "NOT_FOR_THE_HELPER" && v.as_deref() == Some("x"))
        );
        assert_eq!(stale_pattern(&spec(false, true)), STALE_PATTERN);
        assert_eq!(
            line(&command(&spec(false, false))),
            ["/home/u/.config/LIVI/driver/livi-helperd"]
        );
    }

    #[test]
    fn livi_os_lets_through_everything_the_helper_is_given() {
        let mut cfg = defaults();
        cfg.debug_logging = true;
        for (key, _) in helper_env(&cfg, &identity(), "88:00:33:77:8C:26", Some(7000)) {
            assert!(
                crate::livi_os::HELPER_KEYS.contains(&key.as_str()),
                "{key} is missing from livi_os::HELPER_KEYS and docs/LIVI-OS.md"
            );
        }
    }

    #[test]
    fn only_what_the_helper_reads_restarts_it() {
        let before = defaults();
        let mut after = before.clone();
        after.audio_volume = 0.3;
        assert!(!starts_differently(&before, &after));
        after.car_type = Some(CarType::HybridDiesel);
        assert!(starts_differently(&before, &after));
        after.car_type = before.car_type;
        after.wireless_aa_enabled = true;
        assert!(starts_differently(&before, &after));
    }

    #[test]
    fn fuels_follow_the_car_type() {
        assert_eq!(cp_fuels(Some(CarType::E85)), "Gasoline");
        assert_eq!(cp_fuels(Some(CarType::Biodiesel)), "Diesel");
        assert_eq!(cp_fuels(Some(CarType::Electric)), "Electric");
        assert_eq!(cp_fuels(Some(CarType::Cng)), "CNG");
        assert_eq!(cp_fuels(Some(CarType::HybridGasoline)), "Gasoline,Electric");
        assert_eq!(cp_fuels(Some(CarType::HybridDiesel)), "Diesel,Electric");
        assert_eq!(cp_fuels(Some(CarType::Lpg)), "");
        assert_eq!(cp_fuels(None), "");
    }

    #[test]
    fn staging_copies_once_and_replaces_a_changed_binary() {
        let dir = TempDir::new();
        let src = dir.0.join("src-helper");
        let dest = dir.0.join("driver/livi-helperd");
        fs::write(&src, b"v1").unwrap();
        stage(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v1");
        assert_eq!(
            fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o755
        );
        stage(&src, &dest).unwrap();
        fs::write(&src, b"v2").unwrap();
        stage(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v2");
    }

    fn script(dir: &TempDir, body: &str) -> PathBuf {
        let path = dir.0.join("fake-helper");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn a_dying_helper_is_restarted_up_to_the_limit() {
        let dir = TempDir::new();
        let count = dir.0.join("starts");
        let bin = script(&dir, &format!("echo x >> {}", count.display()));
        let sup = HelperSupervisor::start(Spec {
            bin,
            env: Box::new(Vec::new),
            restart: Arc::new(Notify::new()),
            use_sudo: false,
            livi_os: false,
            restart_delay: Duration::from_millis(10),
            max_restarts: Some(2),
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            while fs::read_to_string(&count)
                .map(|s| s.lines().count())
                .unwrap_or(0)
                < 3
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(fs::read_to_string(&count).unwrap().lines().count(), 3);
        sup.stop().await;
    }

    #[tokio::test]
    async fn stop_ends_a_running_helper() {
        let dir = TempDir::new();
        let bin = script(&dir, "exec sleep 30");
        let sup = HelperSupervisor::start(Spec {
            bin,
            env: Box::new(|| vec![("LIVI_TEST".into(), "1".into())]),
            restart: Arc::new(Notify::new()),
            use_sudo: false,
            livi_os: false,
            restart_delay: Duration::from_secs(5),
            max_restarts: None,
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        tokio::time::timeout(Duration::from_secs(5), sup.stop())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn new_settings_start_the_helper_again_at_once() {
        let dir = TempDir::new();
        let count = dir.0.join("starts");
        let bin = script(
            &dir,
            &format!("echo $LIVI_TEST >> {}\nexec sleep 30", count.display()),
        );
        let restart = Arc::new(Notify::new());
        let n = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let built = n.clone();
        let sup = HelperSupervisor::start(Spec {
            bin,
            env: Box::new(move || {
                let i = built.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                vec![("LIVI_TEST".into(), i.to_string())]
            }),
            restart: restart.clone(),
            use_sudo: false,
            livi_os: false,
            restart_delay: Duration::from_secs(60),
            max_restarts: Some(0),
        });
        let lines = |want: usize| {
            let count = count.clone();
            async move {
                while fs::read_to_string(&count)
                    .map(|s| s.lines().count())
                    .unwrap_or(0)
                    < want
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), lines(1))
            .await
            .unwrap();
        restart.notify_one();
        tokio::time::timeout(Duration::from_secs(5), lines(2))
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(&count).unwrap(), "0\n1\n");
        tokio::time::timeout(Duration::from_secs(5), sup.stop())
            .await
            .unwrap();
    }
}
