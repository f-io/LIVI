// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs::{self, File, OpenOptions, TryLockError};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use livi_core_proto::state::{Front, PerScreen, State};
use tokio::net::UnixListener;
use tokio::sync::Notify;

mod aa_telemetry;
mod android_auto;
#[cfg_attr(target_os = "android", path = "audio_devices/android.rs")]
mod audio_devices;
mod blinker;
mod camera;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod car_bridge;
mod carplay;
mod child;
mod config_file;
mod cp_radio;
mod cp_telemetry;
mod devices;
mod dongle;
mod fonts;
mod gnss;
mod gvfs_guard;
mod hands_free;
mod helper;
mod host;
mod host_output;
mod host_setup;
mod http;
mod hub;
mod hwaddr;
mod kiosk_unit;
pub mod log_file;
mod mfi;
mod nav_text;
mod nmea;
pub mod paths;
pub mod power;
mod privileged;
mod projection;
mod radio;
pub mod resources;
mod server;
mod services;
mod spectrum;
mod state_files;
mod status_file;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod system_volume;
mod telemetry;
pub mod telemetry_cli;
mod theme_catalog;
mod udev_rule;
mod ui;
mod update;
mod wifi_ap;
mod wifi_options;

use config_file::{CAR_NAME_MAX, ConfigFile, HostFacts};
use devices::Shared;
pub use helper::Helper;
#[cfg(target_os = "android")]
pub use host::set_screen;
use hub::Hub;
#[cfg(target_os = "android")]
pub use hwaddr::set_bt_mac;
pub use livi_media::gst_host::{EmbeddedHost, MediaHost};
pub use paths::Paths;
pub use resources::Resources;
use server::Core;
use services::Services;
pub use update::{Install, Installer};

pub enum Exit {
    Quit,
    PowerOff,
    Restart(Option<PathBuf>),
}

pub struct Instance {
    paths: Paths,
    _lock: File,
}

impl Instance {
    pub fn claim(paths: Paths) -> Result<Self, String> {
        private_dir(&paths.runtime)?;
        let lock = lock_instance(&paths.lock())?;
        if let Some(user_data) = paths.config.parent() {
            log_file::start(&user_data.join("log"));
        }
        Ok(Self { paths, _lock: lock })
    }
}

async fn goodbye(core: &Core) {
    let _ = tokio::time::timeout(Duration::from_secs(3), core.goodbye()).await;
    let _ = tokio::time::timeout(Duration::from_secs(4), core.shared.clone().leave_phones()).await;
}

pub async fn run(
    instance: Instance,
    res: Resources,
    stop: impl Future<Output = ()>,
) -> Result<Exit, String> {
    let uid = unsafe { libc::geteuid() };
    let paths = &instance.paths;

    let host = HostFacts {
        car_name: host::hostname().and_then(|h| host::car_name(&h, CAR_NAME_MAX)),
        panel_px: host::panel_native_px(),
    };
    let config_file = ConfigFile::new(paths.config.clone(), paths.config_backup.clone());
    let config = config_file.load(&host);
    let user_data = paths
        .config
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let saved = state_files::saved(&user_data);
    let resume_radio = saved.radio;
    let start = projection::start_page(&config, saved.ui_path.as_deref());
    let wants = projection::first_wants(&start);
    let livi = PerScreen {
        main: Front::Livi,
        cluster: Front::Livi,
        aux: Front::Livi,
    };
    let quit = Arc::new(Notify::new());
    let hub = Hub::new(State {
        front: livi,
        sessions: Default::default(),
        now_playing: Default::default(),
        video_playback: None,
        telemetry: Default::default(),
        navigation: Default::default(),
        system: livi_core_proto::state::System {
            start_page: start,
            ..Default::default()
        },
        devices: Default::default(),
        update: Default::default(),
        cluster: Default::default(),
        dab: Default::default(),
        fm: Default::default(),
        cameras: Vec::new(),
        mfi: Default::default(),
        config,
    });
    let (asks_tx, asks, dongle_asks, update_asks) = projection::ui_channels(wants);
    let core = Arc::new(
        Core::new(hub, config_file, uid, quit.clone(), asks_tx)
            .with_mfi(user_data.join("mfi"))
            .with_shared(Shared::new(&paths.helper.shared)),
    );
    if let Err(e) = core.refresh_mfi().await {
        eprintln!("[core] mfi files: {e}");
    }

    let socket = paths.socket();
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)
        .map_err(|e| format!("cannot listen on {}: {e}", socket.display()))?;
    println!("[core] listening on {}", socket.display());

    tokio::spawn(dongle::run(core.hub.clone(), dongle_asks));
    tokio::spawn(wifi_options::follow(core.clone()));
    let (inside, inside_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(telemetry::run(core.clone(), inside_rx, saved));
    tokio::spawn(gnss::run(core.clone(), inside.clone()));
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    tokio::spawn(car_bridge::run(core.clone(), inside.clone()));
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    tokio::spawn(system_volume::follow(core.clone()));
    tokio::spawn(update::run(
        core.clone(),
        update_asks,
        res.installer.clone(),
    ));
    tokio::spawn(fonts::follow(core.clone(), user_data.join("fonts")));
    state_files::start(&core, core.activity(), &user_data);
    tokio::spawn(audio_devices::follow(
        core.hub.clone(),
        res.gstreamer.clone(),
        core.shared.clone(),
    ));
    host_setup::prepare();
    let privileged = res
        .helper_bin()
        .zip(res.templates.clone())
        .map(|(helper, templates)| privileged::Privileged {
            templates,
            helper: helper::resolve_bin(helper, &user_data),
            helper_named: user_data.join("driver/livi-helperd"),
            user_data: user_data.clone(),
        });
    if let Some(privileged) = privileged {
        if udev_rule::ensure(&privileged).await {
            return Ok(Exit::Restart(None));
        }
        if kiosk_unit::ensure(&privileged).await {
            return Ok(Exit::Quit);
        }
        tokio::spawn(gvfs_guard::start(privileged.clone()));
        tokio::spawn(wifi_ap::follow(core.clone(), privileged));
    }
    host_output::prepare(&core.hub).await;
    let (settings, mut patches) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
    let saving = core.clone();
    tokio::spawn(async move {
        while let Some(patch) = patches.recv().await {
            if let Err(e) = saving.set_config(&patch).await {
                eprintln!("[core] setting from the phone not saved: {e}");
            }
        }
    });
    let services = Services::start(
        res,
        paths,
        &user_data,
        core.hub.clone(),
        quit.clone(),
        asks,
        inside,
        settings,
        resume_radio,
    );

    tokio::select! {
        _ = server::serve(core.clone(), listener) => {}
        _ = stop => {}
        _ = quit.notified() => println!("[core] stopping"),
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), gvfs_guard::stop()).await;
    goodbye(&core).await;
    services.stop().await;
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        wifi_ap::release_for_quit(&core.hub.config()),
    )
    .await;
    let _ = fs::remove_file(&socket);
    Ok(if core.restart.load(Ordering::SeqCst) {
        Exit::Restart(
            core.relaunch
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take(),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;

    #[test]
    fn a_second_core_finds_the_lock_taken() {
        let dir = TempDir::new();
        let path = dir.0.join("core.lock");
        let first = lock_instance(&path).unwrap();
        assert_eq!(
            lock_instance(&path).err().as_deref(),
            Some("another livi-core is running")
        );
        drop(first);
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
