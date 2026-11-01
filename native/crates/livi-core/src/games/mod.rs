// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod controllers;
pub mod import;
pub mod launch;
pub mod library;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::config::GamesConfig;
use livi_core_proto::state::{GameImport, GameRun};
use livi_media::compositor::Ui;
use tokio::process::Child;
use tokio::sync::{mpsc, watch};

use crate::hub::Hub;
use library::{Dirs, Game};

const STOP_GRACE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamesAsk {
    Refresh,
    Import,
    Launch(String),
    OpenRetroArch,
    Stop,
    ListControllers,
    ScanControllers,
    Pair(String),
}

pub type GamesAsks = mpsc::UnboundedReceiver<GamesAsk>;

enum Done {
    Scanned(Vec<Game>),
    Imported(Result<GameImport, String>),
}

struct Games {
    hub: Arc<Hub>,
    display: Option<watch::Receiver<Option<Ui>>>,
    library: HashMap<String, Game>,
    child: Option<Child>,
    importing: bool,
    done: mpsc::UnboundedSender<Done>,
    fetch: Fetch,
}

type Fetch = fn(String) -> std::pin::Pin<Box<dyn Future<Output = Option<Vec<u8>>> + Send>>;

fn http(url: String) -> std::pin::Pin<Box<dyn Future<Output = Option<Vec<u8>>> + Send>> {
    Box::pin(import::http_fetch(url))
}

async fn exited(child: &mut Option<Child>) -> std::io::Result<std::process::ExitStatus> {
    match child {
        Some(c) => c.wait().await,
        None => std::future::pending().await,
    }
}

pub async fn run(hub: Arc<Hub>, asks: GamesAsks, display: Option<watch::Receiver<Option<Ui>>>) {
    run_with(hub, asks, display, http).await;
}

async fn run_with(
    hub: Arc<Hub>,
    mut asks: GamesAsks,
    display: Option<watch::Receiver<Option<Ui>>>,
    fetch: Fetch,
) {
    let (done, mut finished) = mpsc::unbounded_channel();
    let mut state = hub.watch();
    let mut seen = state.borrow().config.games.clone();
    let mut games = Games {
        hub,
        display,
        library: HashMap::new(),
        child: None,
        importing: false,
        done,
        fetch,
    };
    games.refresh();
    loop {
        tokio::select! {
            ask = asks.recv() => match ask {
                Some(ask) => games.ask(ask).await,
                None => break,
            },
            Some(done) = finished.recv() => games.finished(done),
            status = exited(&mut games.child) => {
                games.child = None;
                println!("[games] RetroArch ended: {status:?}");
                games.hub.update(|s| s.games.run = GameRun::Idle);
            }
            changed = state.changed() => {
                if changed.is_err() {
                    break;
                }
                let now = state.borrow_and_update().config.games.clone();
                if now != seen {
                    seen = now;
                    games.refresh();
                }
            }
        }
    }
    if let Some(mut child) = games.child.take() {
        crate::child::terminate(&mut child, STOP_GRACE, true).await;
    }
}

impl Games {
    fn config(&self) -> GamesConfig {
        self.hub.config().games
    }

    fn fail(&self, error: impl Into<String>) {
        let error = error.into();
        eprintln!("[games] {error}");
        self.hub.update(|s| s.games.error = Some(error));
    }

    fn refresh(&self) {
        let cfg = self.config();
        if !cfg.enabled {
            let _ = self.done.send(Done::Scanned(Vec::new()));
            return;
        }
        let dirs = Dirs::of(&cfg);
        let done = self.done.clone();
        tokio::task::spawn_blocking(move || {
            let _ = done.send(Done::Scanned(library::scan(&dirs)));
        });
    }

    fn finished(&mut self, done: Done) {
        match done {
            Done::Scanned(list) => {
                let views = list.iter().map(Game::view).collect();
                self.library = list.into_iter().map(|g| (g.id.clone(), g)).collect();
                self.hub.update(|s| {
                    s.games.library = views;
                    s.games.scanned = true;
                });
            }
            Done::Imported(result) => {
                self.importing = false;
                self.hub.update(|s| {
                    s.games.importing = false;
                    match result {
                        Ok(summary) => s.games.last_import = Some(summary),
                        Err(e) => s.games.error = Some(e),
                    }
                });
                self.refresh();
            }
        }
    }

    async fn ask(&mut self, ask: GamesAsk) {
        let cfg = self.config();
        let needs_games = !matches!(
            ask,
            GamesAsk::Refresh
                | GamesAsk::Stop
                | GamesAsk::ListControllers
                | GamesAsk::ScanControllers
                | GamesAsk::Pair(_)
        );
        if needs_games && !cfg.enabled {
            return self.fail("Games screen is disabled");
        }
        match ask {
            GamesAsk::Refresh => {
                self.hub.update(|s| s.games.error = None);
                self.refresh();
            }
            GamesAsk::Import => self.import(&cfg),
            GamesAsk::Launch(id) => {
                let game = match self.library.get(&id) {
                    Some(g) => Some(g.clone()),
                    None => {
                        let dirs = Dirs::of(&cfg);
                        tokio::task::spawn_blocking(move || library::scan(&dirs))
                            .await
                            .unwrap_or_default()
                            .into_iter()
                            .find(|g| g.id == id)
                    }
                };
                match game {
                    Some(game) => self.start(&cfg, launch::game_args(&game), Some(game.id)),
                    None => self.fail("Game is no longer in library"),
                }
            }
            GamesAsk::OpenRetroArch => self.start(&cfg, launch::menu_args(), None),
            GamesAsk::Stop => {
                if let Some(child) = self.child.as_mut() {
                    crate::child::terminate(child, STOP_GRACE, true).await;
                }
            }
            GamesAsk::ListControllers => self.controllers(Bt::List),
            GamesAsk::ScanControllers => self.controllers(Bt::Scan),
            GamesAsk::Pair(mac) => self.controllers(Bt::Pair(mac)),
        }
    }

    fn import(&mut self, cfg: &GamesConfig) {
        // A second press while one runs joins the running import.
        if std::mem::replace(&mut self.importing, true) {
            return;
        }
        self.hub.update(|s| {
            s.games.importing = true;
            s.games.error = None;
            s.games.last_import = None;
        });
        let (dirs, done, fetch) = (Dirs::of(cfg), self.done.clone(), self.fetch);
        tokio::spawn(async move {
            let _ = done.send(Done::Imported(import::import(dirs, fetch).await));
        });
    }

    fn start(&mut self, cfg: &GamesConfig, rest: Vec<String>, game: Option<String>) {
        if self.child.is_some() {
            return self.fail("RetroArch is already running");
        }
        let exe = cfg.retro_arch_path.trim();
        if exe.is_empty() {
            return self.fail("RetroArch executable is not configured");
        }
        let size = launch::main_size(&self.hub.watch().borrow());
        let args = launch::args(size, rest);
        self.hub.update(|s| {
            s.games.run = GameRun::Launching;
            s.games.game_id = game.clone();
            s.games.error = None;
        });
        let ui = self.display.as_ref().and_then(|d| d.borrow().clone());
        println!("[games] starting {exe} {}", args.join(" "));
        match launch::spawn(exe, &args, ui.as_ref()) {
            Ok(child) => {
                // LIVI stays mapped behind RetroArch, the compositor raises the game above it.
                self.child = Some(child);
                self.hub.update(|s| s.games.run = GameRun::Running);
            }
            Err(e) => {
                self.hub.update(|s| s.games.run = GameRun::Idle);
                self.fail(format!("Could not start RetroArch: {e}"));
            }
        }
    }

    fn controllers(&self, what: Bt) {
        let busy = {
            let s = self.hub.watch();
            let g = &s.borrow().games;
            g.scanning || g.pairing.is_some()
        };
        if busy {
            return;
        }
        let hub = self.hub.clone();
        let adapter = self.hub.config().bt_adapter;
        tokio::spawn(async move {
            hub.update(|s| {
                s.games.controller_error = None;
                match &what {
                    Bt::Scan => s.games.scanning = true,
                    Bt::Pair(mac) => s.games.pairing = Some(mac.to_uppercase()),
                    Bt::List => {}
                }
            });
            let result = bluetooth(&adapter, &what).await;
            hub.update(|s| {
                s.games.scanning = false;
                s.games.pairing = None;
                match result {
                    Ok(list) => s.games.controllers = list,
                    Err(e) => s.games.controller_error = Some(e),
                }
            });
        });
    }
}

enum Bt {
    List,
    Scan,
    Pair(String),
}

async fn bluetooth(
    adapter: &str,
    what: &Bt,
) -> Result<Vec<livi_core_proto::state::ControllerView>, String> {
    if let Bt::Pair(mac) = what {
        controllers::valid_mac(mac)?;
    }
    let bus = controllers::Bluez::open().await?;
    match what {
        Bt::List => bus.list(adapter).await,
        Bt::Scan => bus.scan(adapter).await,
        Bt::Pair(mac) => {
            controllers::pair_with(&bus, adapter, mac).await?;
            bus.list(adapter).await
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use livi_core_proto::state::{Front, PerScreen, State};
    use serde_json::json;

    use super::*;
    use crate::config_file::tests::TempDir;

    pub fn state() -> State {
        let livi = PerScreen {
            main: Front::Livi,
            cluster: Front::Livi,
            aux: Front::Livi,
        };
        State {
            front: livi,
            sessions: Default::default(),
            now_playing: Default::default(),
            telemetry: Default::default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            cluster: Default::default(),
            dab: Default::default(),
            games: Default::default(),
            cameras: Vec::new(),
            mfi: Default::default(),
            config: crate::config_file::defaults(),
        }
    }

    fn no_fetch(_url: String) -> std::pin::Pin<Box<dyn Future<Output = Option<Vec<u8>>> + Send>> {
        Box::pin(async { None })
    }

    struct Rig {
        hub: Arc<Hub>,
        asks: mpsc::UnboundedSender<GamesAsk>,
        dir: TempDir,
    }

    fn rig(exe: &str) -> Rig {
        let dir = TempDir::new();
        let mut s = state();
        s.config.main_screen_width = 800;
        s.config.main_screen_height = 480;
        let root = dir.0.to_string_lossy().into_owned();
        s.config.games = GamesConfig {
            enabled: true,
            retro_arch_path: exe.into(),
            rom_directory: format!("{root}/roms"),
            playlist_directory: format!("{root}/playlists"),
            thumbnail_directory: format!("{root}/thumbnails"),
            cores_directory: format!("{root}/cores"),
        };
        fs::create_dir_all(dir.0.join("playlists")).unwrap();
        let item =
            json!({ "path": "/roms/game.rom", "label": "Game", "core_path": "/cores/core.so" });
        fs::write(
            dir.0.join("playlists/System.lpl"),
            json!({ "items": [item] }).to_string(),
        )
        .unwrap();
        let hub = Arc::new(Hub::new(s));
        let (asks, rx) = mpsc::unbounded_channel();
        tokio::spawn(run_with(hub.clone(), rx, None, no_fetch));
        Rig { hub, asks, dir }
    }

    async fn until(hub: &Hub, done: impl Fn(&State) -> bool) {
        let mut w = hub.watch();
        tokio::time::timeout(Duration::from_secs(10), w.wait_for(|s| done(s)))
            .await
            .expect("never happened")
            .unwrap();
    }

    fn recorder(dir: &TempDir) -> String {
        let script = dir.0.join("retroarch.sh");
        let out = dir.0.join("args.txt");
        fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", out.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        script.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn a_game_runs_in_retroarch_and_livi_comes_back_when_it_closes() {
        let probe = TempDir::new();
        let exe = recorder(&probe);
        let r = rig(&exe);
        until(&r.hub, |s| s.games.library.len() == 1).await;
        let id = r.hub.watch().borrow().games.library[0].id.clone();
        r.asks.send(GamesAsk::Launch(id.clone())).unwrap();
        until(&r.hub, |s| s.games.game_id.as_deref() == Some(id.as_str())).await;
        until(&r.hub, |s| s.games.run == GameRun::Idle).await;
        let args = fs::read_to_string(probe.0.join("args.txt")).unwrap();
        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            [
                "--fullscreen",
                "--size=800x480",
                "-L",
                "/cores/core.so",
                "/roms/game.rom"
            ]
        );
        assert_eq!(r.hub.watch().borrow().games.error, None);
    }

    #[tokio::test]
    async fn the_menu_opens_for_library_setup() {
        let probe = TempDir::new();
        let r = rig(&recorder(&probe));
        r.asks.send(GamesAsk::OpenRetroArch).unwrap();
        for _ in 0..500 {
            if fs::read_to_string(probe.0.join("args.txt")).is_ok_and(|t| t.ends_with("--menu\n")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let args = fs::read_to_string(probe.0.join("args.txt")).unwrap();
        assert_eq!(args, "--fullscreen\n--size=800x480\n--menu\n");
    }

    #[tokio::test]
    async fn a_missing_game_or_executable_is_reported() {
        let r = rig("/nonexistent/retroarch");
        until(&r.hub, |s| s.games.scanned).await;
        r.asks.send(GamesAsk::Launch("gone".into())).unwrap();
        until(&r.hub, |s| {
            s.games.error.as_deref() == Some("Game is no longer in library")
        })
        .await;
        let id = r.hub.watch().borrow().games.library[0].id.clone();
        r.asks.send(GamesAsk::Launch(id)).unwrap();
        until(&r.hub, |s| {
            s.games
                .error
                .as_deref()
                .is_some_and(|e| e.starts_with("Could not start RetroArch"))
        })
        .await;
        assert_eq!(r.hub.watch().borrow().games.run, GameRun::Idle);
    }

    #[tokio::test]
    async fn an_import_runs_once_at_a_time_and_rescans_after() {
        let r = rig("true");
        until(&r.hub, |s| s.games.scanned).await;
        fs::create_dir_all(r.dir.0.join("roms/nes")).unwrap();
        fs::write(r.dir.0.join("roms/nes/Zelda.nes"), "rom").unwrap();
        let mut w = r.hub.watch();
        r.asks.send(GamesAsk::Import).unwrap();
        r.asks.send(GamesAsk::Import).unwrap();
        w.wait_for(|s| s.games.importing).await.unwrap();
        until(&r.hub, |s| {
            !s.games.importing && s.games.library.iter().any(|g| g.title == "Zelda")
        })
        .await;
        let s = r.hub.watch().borrow().clone();
        let summary = s.games.last_import.unwrap();
        assert_eq!(summary.games, 1);
        assert_eq!(summary.playlists, 1);
        assert_eq!(summary.thumbnails_missing, 1);
        r.asks.send(GamesAsk::Import).unwrap();
        until(&r.hub, |s| s.games.importing).await;
        until(&r.hub, |s| !s.games.importing).await;
    }

    #[tokio::test]
    async fn turned_off_games_list_nothing_and_refuse_to_launch() {
        let r = rig("true");
        until(&r.hub, |s| s.games.library.len() == 1).await;
        let mut games = r.hub.config().games;
        games.enabled = false;
        r.hub.update(|s| s.config.games = games);
        until(&r.hub, |s| s.games.library.is_empty()).await;
        r.asks.send(GamesAsk::Import).unwrap();
        until(&r.hub, |s| {
            s.games.error.as_deref() == Some("Games screen is disabled")
        })
        .await;
    }
}
