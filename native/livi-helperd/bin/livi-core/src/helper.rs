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
/// The end anchor spares the access point service, which runs the same binary as --wifi-ap.
const STALE_PATTERN: &str = "driver/livi-helperd$";
const STOP_GRACE: Duration = Duration::from_secs(3);

pub fn debug(cfg: &Config) -> bool {
    cfg.debug_logging || std::env::var("DEBUG").is_ok_and(|v| v == "1")
}

pub fn helper_env(
    cfg: &Config,
    identity: &Identity,
    airplay_port: Option<u16>,
) -> Vec<(String, String)> {
    let mut env = settings_env(cfg);
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

/// The fuels the phone hears ranges for.
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

fn settings_env(cfg: &Config) -> Vec<(String, String)> {
    let flag = |on: bool, off: &str| if on { "1".to_string() } else { off.to_string() };
    let debug = debug(cfg);
    let car_name = if cfg.car_name.is_empty() {
        std::env::var("LIVI_CP_NAME").unwrap_or_default()
    } else {
        cfg.car_name.clone()
    };
    let channel = if cfg.wifi_channel == 0 { String::new() } else { cfg.wifi_channel.to_string() };
    vec![
        ("LIVI_AA_WIRELESS".to_string(), flag(cfg.wireless_aa_enabled, "0")),
        ("LIVI_CP_WIRELESS".to_string(), flag(cfg.wireless_cp_enabled, "")),
        ("DEBUG".to_string(), flag(debug, "")),
        ("LIVI_CP_NAME".to_string(), car_name),
        ("LIVI_CP_FUELS".to_string(), cp_fuels(cfg.car_type).to_string()),
        ("LIVI_BT_ADAPTER".to_string(), cfg.bt_adapter.clone()),
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
    let appdir = std::env::var_os("APPDIR").map(PathBuf::from).unwrap_or_default();
    (std::env::var_os("APPIMAGE").is_some() && p.starts_with(&appdir))
        || p.to_string_lossy().contains("/.mount_")
}

fn digest(p: &Path) -> io::Result<Vec<u8>> {
    Ok(Sha256::digest(fs::read(p)?).to_vec())
}

/// Root cannot run a binary from the user's FUSE mount, so it is copied out.
/// Temp file plus rename, because the access point service may be running the
/// old one.
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

/// `user_data` is where the sudoers rule points.
pub fn resolve_bin(bin: &Path, user_data: &Path) -> PathBuf {
    if !cfg!(target_os = "linux") || !bin.exists() || !inside_appimage_mount(bin) {
        return bin.to_path_buf();
    }
    let staged = user_data.join("driver").join(HELPER_BIN);
    match stage(bin, &staged) {
        Ok(()) => staged,
        Err(e) => {
            eprintln!("[core] staging the helper failed: {e}");
            // Even an older staged copy is the only one root can run.
            if staged.exists() { staged } else { bin.to_path_buf() }
        }
    }
}

/// A crashed LIVI leaves a root helper holding the MFi GPIO, the RFCOMM channel
/// and the BT sockets, and only root can end it.
fn kill_stale() {
    let running = std::process::Command::new("pgrep")
        .args(["-f", STALE_PATTERN])
        .output()
        .is_ok_and(|o| !o.stdout.is_empty());
    if running {
        println!("[core] stopping a stale helper");
        let _ = std::process::Command::new("sudo")
            .args(["-n", "pkill", "-f", STALE_PATTERN])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub struct Spec {
    pub bin: PathBuf,
    /// Built at every start, so a restart takes the config as it is then.
    pub env: Box<dyn Fn() -> Vec<(String, String)> + Send + Sync>,
    pub restart: Arc<Notify>,
    pub use_sudo: bool,
    pub restart_delay: Duration,
    /// None restarts for ever.
    pub max_restarts: Option<u32>,
}

pub struct HelperSupervisor {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl HelperSupervisor {
    pub fn start(spec: Spec) -> Self {
        let (stop, stop_rx) = watch::channel(false);
        Self { stop, task: tokio::spawn(run(spec, stop_rx)) }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

fn spawn(spec: &Spec) -> io::Result<Child> {
    let mut cmd = if spec.use_sudo {
        let mut c = Command::new("sudo");
        c.arg("-n").arg("-E").arg(&spec.bin);
        c
    } else {
        Command::new(&spec.bin)
    };
    if let Some(dir) = spec.bin.parent() {
        cmd.current_dir(dir);
    }
    crate::child::with_lifeline(&mut cmd)
        .envs((spec.env)())
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
        if cfg!(target_os = "linux") && spec.use_sudo {
            kill_stale();
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
        Identity { secret: [1; 32], public: [0xab; 32], pairing_id: "pi-1".into() }
    }

    fn get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn env_carries_the_config_and_identity() {
        let mut cfg = defaults();
        cfg.wireless_aa_enabled = true;
        cfg.wifi_type = WifiBand::Ghz24;
        let env = helper_env(&cfg, &identity(), Some(7000));
        assert_eq!(get(&env, "LIVI_AA_WIRELESS"), Some("1"));
        assert_eq!(get(&env, "LIVI_CP_WIRELESS"), Some(""));
        assert_eq!(get(&env, "LIVI_CP_PK"), Some("ab".repeat(32).as_str()));
        assert_eq!(get(&env, "LIVI_CP_PI"), Some("pi-1"));
        assert_eq!(get(&env, "LIVI_CP_NAME"), Some("LIVI"));
        assert_eq!(get(&env, "LIVI_CP_FUELS"), Some("Gasoline"));
        assert_eq!(get(&env, "LIVI_CHANNEL"), Some("36"));
        assert_eq!(get(&env, "LIVI_WIFI_TYPE"), Some("2.4ghz"));
        assert_eq!(get(&env, "LIVI_CP_AIRPLAY_PORT"), Some("7000"));
        cfg.debug_logging = true;
        let env = helper_env(&cfg, &identity(), None);
        assert_eq!(get(&env, "LIVI_CP_AIRPLAY_PORT"), None);
        assert_eq!(get(&env, "DEBUG"), Some("1"));
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
        assert_eq!(fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o755);
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
            restart_delay: Duration::from_millis(10),
            max_restarts: Some(2),
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            while fs::read_to_string(&count).map(|s| s.lines().count()).unwrap_or(0) < 3 {
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
            restart_delay: Duration::from_secs(5),
            max_restarts: None,
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        tokio::time::timeout(Duration::from_secs(5), sup.stop()).await.unwrap();
    }

    #[tokio::test]
    async fn new_settings_start_the_helper_again_at_once() {
        let dir = TempDir::new();
        let count = dir.0.join("starts");
        let bin = script(&dir, &format!("echo $LIVI_TEST >> {}\nexec sleep 30", count.display()));
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
            restart_delay: Duration::from_secs(60),
            max_restarts: Some(0),
        });
        let lines = |want: usize| {
            let count = count.clone();
            async move {
                while fs::read_to_string(&count).map(|s| s.lines().count()).unwrap_or(0) < want {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), lines(1)).await.unwrap();
        restart.notify_one();
        tokio::time::timeout(Duration::from_secs(5), lines(2)).await.unwrap();
        assert_eq!(fs::read_to_string(&count).unwrap(), "0\n1\n");
        tokio::time::timeout(Duration::from_secs(5), sup.stop()).await.unwrap();
    }
}
