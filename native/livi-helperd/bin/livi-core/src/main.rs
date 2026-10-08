use std::fs::{self, File, OpenOptions, TryLockError};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use livi_core_proto::LIFELINE_ENV;
use livi_core_proto::state::{Front, PerScreen, State};
use tokio::net::UnixListener;
use tokio::sync::Notify;

mod aa_telemetry;
mod android_auto;
mod audio_devices;
mod blinker;
mod car_bridge;
mod carplay;
mod child;
mod config_file;
mod cp_telemetry;
mod devices;
mod dongle;
mod gnss;
mod gvfs_guard;
mod hands_free;
mod helper;
mod host;
mod host_output;
mod host_setup;
mod hub;
mod hwaddr;
mod log_file;
mod nav_text;
mod nmea;
mod paths;
mod power;
mod privileged;
mod projection;
mod resources;
mod server;
mod services;
mod spectrum;
mod status_file;
mod system_volume;
mod telemetry;
mod udev_rule;
mod ui;
mod update;
mod wifi_ap;
mod wifi_options;

use config_file::{CAR_NAME_MAX, ConfigFile, HostFacts};
use hub::Hub;
use paths::Paths;
use resources::Resources;
use server::Core;
use services::Services;

fn main() -> ExitCode {
    if let Some(arg) = std::env::args().nth(1) {
        eprintln!("livi-core: unknown argument {arg}, livi-core takes none");
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[core] no runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let outcome = runtime.block_on(run());
    // Dropping the runtime stops the processes that end with their handles.
    runtime.shutdown_timeout(Duration::from_secs(2));
    let code = match outcome {
        Ok(Exit::Quit) => ExitCode::SUCCESS,
        Ok(Exit::PowerOff) => {
            power::run(power::Power::Off);
            ExitCode::SUCCESS
        }
        Ok(Exit::Restart(_)) if power::owns_host() => {
            power::run(power::Power::Reboot);
            ExitCode::SUCCESS
        }
        Ok(Exit::Restart(Some(_))) if cfg!(target_os = "macos") => {
            println!("[core] the app starts again");
            ExitCode::from(RELAUNCH_THE_APP)
        }
        Ok(Exit::Restart(program)) => restart(program),
        Err(e) => {
            eprintln!("[core] {e}");
            ExitCode::FAILURE
        }
    };
    log_file::finish();
    code
}

/// The macOS app starts again from its freshly updated bundle on this exit code.
const RELAUNCH_THE_APP: u8 = 75;

enum Exit {
    Quit,
    PowerOff,
    Restart(Option<PathBuf>),
}

/// The phones get their goodbye while the helper still carries them.
async fn goodbye(core: &Core) {
    let _ = tokio::time::timeout(Duration::from_secs(3), core.goodbye()).await;
    let _ = tokio::time::timeout(Duration::from_secs(4), devices::leave_phones()).await;
}

/// Execs in this process, so an AppImage stays mounted.
fn restart(program: Option<PathBuf>) -> ExitCode {
    println!("[core] restarting");
    log_file::finish();
    let err = match program.map_or_else(std::env::current_exe, Ok) {
        Ok(exe) => std::process::Command::new(exe).exec(),
        Err(e) => e,
    };
    eprintln!("[core] cannot restart: {err}");
    ExitCode::FAILURE
}

async fn run() -> Result<Exit, String> {
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let paths = Paths::from_env(uid)?;
    private_dir(&paths.runtime)?;
    let _instance = lock_instance(&paths.lock())?;
    if let Some(user_data) = paths.config.parent() {
        log_file::start(&user_data.join("log"));
    }

    let host = HostFacts {
        car_name: host::hostname().and_then(|h| host::car_name(&h, CAR_NAME_MAX)),
        panel_px: host::panel_native_px(),
    };
    let config_file = ConfigFile::new(paths.config.clone(), paths.config_backup.clone());
    let config = config_file.load(&host);
    let livi = PerScreen { main: Front::Livi, dash: Front::Livi, aux: Front::Livi };
    let quit = Arc::new(Notify::new());
    if std::env::var(LIFELINE_ENV).is_ok_and(|v| v == "1") {
        let quit = quit.clone();
        log_file::follow_ui(move || {
            println!("[core] the UI that started core is gone");
            quit.notify_one();
        });
    }
    let hub = Hub::new(State {
        front: livi,
        sessions: Default::default(),
        now_playing: Default::default(),
        telemetry: Default::default(),
        navigation: Default::default(),
        system: Default::default(),
        devices: Default::default(),
        update: Default::default(),
        config,
    });
    let (asks_tx, asks, dongle_asks, update_asks) = projection::ui_channels(livi);
    let core = Arc::new(Core::new(hub, config_file, uid, quit.clone(), asks_tx));

    // Whatever sits there is left over, the lock says no other core runs.
    let socket = paths.socket();
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)
        .map_err(|e| format!("cannot listen on {}: {e}", socket.display()))?;
    println!("[core] listening on {}", socket.display());

    let user_data = paths.config.parent().map(Path::to_path_buf).unwrap_or_default();
    tokio::spawn(dongle::run(core.hub.clone(), dongle_asks));
    tokio::spawn(wifi_options::follow(core.clone()));
    let (inside, inside_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(telemetry::run(core.clone(), inside_rx));
    tokio::spawn(gnss::run(core.clone(), inside.clone(), user_data.clone()));
    tokio::spawn(car_bridge::run(core.clone(), inside));
    tokio::spawn(system_volume::follow(core.clone()));
    tokio::spawn(update::run(core.clone(), update_asks));
    tokio::spawn(status_file::run(core.clone(), core.activity(), user_data.clone()));
    let res = Resources::from_env();
    tokio::spawn(audio_devices::follow(core.hub.clone(), res.gstreamer.clone()));
    host_setup::prepare();
    let privileged = privileged::Privileged {
        templates: res.templates.clone(),
        helper: helper::resolve_bin(&res.helper, &user_data),
        helper_named: user_data.join("driver/livi-helperd"),
        user_data: user_data.clone(),
    };
    if udev_rule::ensure(&privileged).await {
        return Ok(Exit::Restart(None));
    }
    tokio::spawn(gvfs_guard::start(privileged.clone()));
    tokio::spawn(wifi_ap::follow(core.clone(), privileged));
    host_output::prepare(&core.hub).await;
    let services = Services::start(
        res,
        &paths.runtime,
        &user_data,
        core.hub.clone(),
        socket.clone(),
        quit.clone(),
        asks,
    );

    tokio::select! {
        _ = server::serve(core.clone(), listener) => {}
        _ = shutdown_signal() => {}
        _ = quit.notified() => println!("[core] stopping"),
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), gvfs_guard::stop()).await;
    goodbye(&core).await;
    services.stop().await;
    let _ =
        tokio::time::timeout(Duration::from_secs(2), wifi_ap::release_for_quit(&core.hub.config()))
            .await;
    let _ = fs::remove_file(&socket);
    Ok(if core.restart.load(Ordering::SeqCst) {
        Exit::Restart(core.relaunch.lock().unwrap_or_else(|e| e.into_inner()).take())
    } else if core.quit_asked.load(Ordering::SeqCst) && power::owns_host() {
        Exit::PowerOff
    } else {
        Exit::Quit
    })
}

fn private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("cannot restrict {}: {e}", dir.display()))
}

/// Held for the life of the process, the kernel drops it with us.
fn lock_instance(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err("another livi-core is running".into()),
        Err(TryLockError::Error(e)) => Err(format!("cannot lock {}: {e}", path.display())),
    }
}

async fn shutdown_signal() {
    let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;

    #[test]
    fn a_second_core_finds_the_lock_taken() {
        let dir = TempDir::new();
        let path = dir.0.join("core.lock");
        let first = lock_instance(&path).unwrap();
        assert_eq!(lock_instance(&path).err().as_deref(), Some("another livi-core is running"));
        drop(first);
        // A child that another test forks right now holds the descriptor until its exec.
        let relocked = (0..100).any(|_| {
            let ok = lock_instance(&path).is_ok();
            if !ok {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            ok
        });
        assert!(relocked);
    }

    #[test]
    fn the_runtime_dir_is_private() {
        let dir = TempDir::new();
        let runtime = dir.0.join("run/livi");
        private_dir(&runtime).unwrap();
        let mode = fs::metadata(&runtime).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
