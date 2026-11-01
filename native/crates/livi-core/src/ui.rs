// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use livi_media::compositor::Ui;
use tokio::process::Command;
use tokio::sync::{Notify, watch};

use crate::resources::UiCmd;

const RESTART_DELAY: Duration = Duration::from_secs(1);
const MAX_RESTARTS: u32 = 5;
const STABLE_AFTER: Duration = Duration::from_secs(30);
const STOP_GRACE: Duration = Duration::from_secs(5);

pub struct UiSupervisor {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl UiSupervisor {
    pub fn start(
        cmd: UiCmd,
        core_socket: PathBuf,
        display: Option<watch::Receiver<Option<Ui>>>,
        quit: Arc<Notify>,
    ) -> Self {
        Self::with_delay(cmd, core_socket, display, quit, RESTART_DELAY)
    }

    fn with_delay(
        cmd: UiCmd,
        core_socket: PathBuf,
        display: Option<watch::Receiver<Option<Ui>>>,
        quit: Arc<Notify>,
        restart_delay: Duration,
    ) -> Self {
        let (stop, stop_rx) = watch::channel(false);
        let task = tokio::spawn(run(cmd, core_socket, display, quit, restart_delay, stop_rx));
        Self { stop, task }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

fn command(cmd: &UiCmd, core_socket: &Path, ui: Option<&Ui>) -> Command {
    let mut c = match cmd {
        UiCmd::Exec { bin, args } => {
            let mut c = Command::new(bin);
            c.args(args);
            c
        }
        UiCmd::Shell(line) => {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(line);
            c
        }
    };
    c.env(livi_core_proto::runtime::SOCKET_ENV, core_socket);
    if let Some(ui) = ui {
        c.env("WAYLAND_DISPLAY", &ui.wayland_display)
            .env("XDG_SESSION_TYPE", "wayland")
            .env("LIVI_COMPOSITOR", "1")
            .env_remove("DISPLAY");
        match &ui.render_node {
            Some(node) => c.env("LIVI_RENDER_NODE", node),
            None => c.env_remove("LIVI_RENDER_NODE"),
        };
    }
    crate::child::with_lifeline(&mut c)
        .process_group(0)
        .kill_on_drop(true);
    c
}

async fn run(
    cmd: UiCmd,
    core_socket: PathBuf,
    display: Option<watch::Receiver<Option<Ui>>>,
    quit: Arc<Notify>,
    restart_delay: Duration,
    mut stop: watch::Receiver<bool>,
) {
    let ui = match display {
        Some(mut display) => tokio::select! {
            shown = display.wait_for(Option::is_some) => match shown {
                Ok(ui) => ui.clone(),
                Err(_) => return,
            },
            _ = stop.changed() => return,
        },
        None => None,
    };
    let mut restarts = 0;
    loop {
        let started = Instant::now();
        let mut child = match command(&cmd, &core_socket, ui.as_ref()).spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("[core] cannot start the UI: {e}");
                quit.notify_one();
                return;
            }
        };
        println!("[core] UI started");
        let _lifeline = crate::child::hold_lifeline(&mut child);
        let status = tokio::select! {
            status = child.wait() => status,
            _ = stop.changed() => {
                crate::child::terminate(&mut child, STOP_GRACE, true).await;
                return;
            }
        };
        match status {
            Ok(status) if status.success() => {
                println!("[core] the UI closed");
                quit.notify_one();
                return;
            }
            other => eprintln!("[core] UI exited: {other:?}"),
        }
        if started.elapsed() >= STABLE_AFTER {
            restarts = 0;
        }
        restarts += 1;
        if restarts > MAX_RESTARTS {
            eprintln!("[core] the UI failed {MAX_RESTARTS} times in a row, giving up");
            quit.notify_one();
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep(restart_delay) => {}
            _ = stop.changed() => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn env_of(c: &Command) -> Vec<(String, Option<String>)> {
        c.as_std()
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    fn get(env: &[(String, Option<String>)], key: &str) -> Option<Option<String>> {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn inside_the_compositor_the_ui_gets_its_display() {
        let cmd = UiCmd::Exec {
            bin: PathBuf::from("/a/livi-ui"),
            args: vec!["--x".into()],
        };
        let ui = Ui {
            wayland_display: "wayland-2".into(),
            render_node: None,
        };
        let c = command(&cmd, Path::new("/run/livi/core.sock"), Some(&ui));
        let args: Vec<&OsStr> = c.as_std().get_args().collect();
        assert_eq!(args, ["--x"]);
        let env = env_of(&c);
        assert_eq!(get(&env, "WAYLAND_DISPLAY"), Some(Some("wayland-2".into())));
        assert_eq!(get(&env, "LIVI_COMPOSITOR"), Some(Some("1".into())));
        assert_eq!(get(&env, "DISPLAY"), Some(None));
        assert_eq!(get(&env, "LIVI_RENDER_NODE"), Some(None));
        assert_eq!(
            get(&env, "LIVI_CORE_SOCKET"),
            Some(Some("/run/livi/core.sock".into()))
        );
    }

    #[test]
    fn on_the_desktop_the_ui_keeps_the_session() {
        let c = command(&UiCmd::Shell("./LIVI".into()), Path::new("/s"), None);
        assert_eq!(c.as_std().get_program(), "/bin/sh");
        let args: Vec<&OsStr> = c.as_std().get_args().collect();
        assert_eq!(args, ["-c", "./LIVI"]);
        let env = env_of(&c);
        assert_eq!(get(&env, "WAYLAND_DISPLAY"), None);
        assert_eq!(get(&env, "DISPLAY"), None);
    }

    async fn quits(notify: &Notify) -> bool {
        tokio::time::timeout(Duration::from_secs(5), notify.notified())
            .await
            .is_ok()
    }

    #[tokio::test]
    async fn a_ui_that_closes_takes_core_along() {
        let quit = Arc::new(Notify::new());
        let ui = UiSupervisor::start(
            UiCmd::Shell("exit 0".into()),
            PathBuf::from("/s"),
            None,
            quit.clone(),
        );
        assert!(quits(&quit).await);
        ui.stop().await;
    }

    #[tokio::test]
    async fn a_crashing_ui_is_given_up_after_its_restarts() {
        let quit = Arc::new(Notify::new());
        let ui = UiSupervisor::with_delay(
            UiCmd::Shell("exit 3".into()),
            PathBuf::from("/s"),
            None,
            quit.clone(),
            Duration::from_millis(10),
        );
        assert!(quits(&quit).await);
        ui.stop().await;
    }

    #[tokio::test]
    async fn the_ui_waits_for_the_compositor_display() {
        let quit = Arc::new(Notify::new());
        let (display_tx, display) = watch::channel(None);
        let ui = UiSupervisor::start(
            UiCmd::Shell("exit 0".into()),
            PathBuf::from("/s"),
            Some(display),
            quit.clone(),
        );
        let early = tokio::time::timeout(Duration::from_millis(200), quit.notified()).await;
        assert!(early.is_err());
        let _ = display_tx.send(Some(Ui {
            wayland_display: "w".into(),
            render_node: None,
        }));
        assert!(quits(&quit).await);
        ui.stop().await;
    }

    #[tokio::test]
    async fn stop_ends_a_running_ui() {
        let quit = Arc::new(Notify::new());
        let ui = UiSupervisor::start(
            UiCmd::Shell("sleep 30".into()),
            PathBuf::from("/s"),
            None,
            quit.clone(),
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        tokio::time::timeout(Duration::from_secs(5), ui.stop())
            .await
            .unwrap();
    }
}
