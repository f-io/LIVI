// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use livi_core_client::Client;
use livi_core_proto::config::DEFAULT_ROM_DIRECTORY;
use livi_core_proto::message::Action;
use livi_core_proto::state::{GameRun, Games};
use livi_ui_model::games;
use serde_json::Value;
use slint::{ComponentHandle, Image, Model, ModelRc, SharedPixelBuffer, VecModel};

use crate::media::Pixels;
use crate::{AppWindow, ControllerData, Focus, FocusZone, GameData, GamesState, GamesText, Page};

const COVER_MAX_PX: u32 = 480;
const HEADER: usize = 3;

pub struct GamesView {
    pub games: Games,
    pub covers: Vec<Option<Pixels>>,
}

#[derive(Default)]
pub struct CoverReader {
    covers: HashMap<String, Option<Pixels>>,
}

fn load(path: &str) -> Option<Pixels> {
    let img = image::open(path).ok()?;
    let img = if img.height() > COVER_MAX_PX || img.width() > COVER_MAX_PX {
        img.thumbnail(COVER_MAX_PX, COVER_MAX_PX)
    } else {
        img
    }
    .into_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        img.as_raw(),
        img.width(),
        img.height(),
    ))
}

impl CoverReader {
    pub fn read(&mut self, state: &Value) -> GamesView {
        let games = games::read(state);
        let wanted: Vec<&str> = games
            .library
            .iter()
            .filter_map(|g| g.thumbnail.as_deref())
            .collect();
        self.covers
            .retain(|path, _| wanted.contains(&path.as_str()));
        let covers = games
            .library
            .iter()
            .map(|g| {
                let path = g.thumbnail.as_deref()?;
                self.covers
                    .entry(path.to_string())
                    .or_insert_with(|| load(path))
                    .clone()
            })
            .collect();
        GamesView { games, covers }
    }
}

thread_local! {
    static LAST_RUN: Cell<GameRun> = const { Cell::new(GameRun::Idle) };
}

pub fn apply(ui: &AppWindow, v: GamesView) {
    let s = ui.global::<GamesState>();
    let g = v.games;
    let tr = crate::settings_ui::tr;
    let list: Vec<GameData> = g
        .library
        .iter()
        .zip(v.covers)
        .map(|(game, cover)| GameData {
            id: game.id.clone().into(),
            title: game.title.clone().into(),
            system: game.system.clone().into(),
            has_cover: cover.is_some(),
            cover: cover.map(Image::from_rgba8).unwrap_or_default(),
        })
        .collect();
    ui.global::<GamesText>()
        .set_count(games::count(list.len(), &tr).into());
    s.set_games(ModelRc::from(Rc::new(VecModel::from(list))));
    s.set_scanned(g.scanned);
    s.set_importing(g.importing);
    s.set_busy(games::busy(&g));
    let launching = g.game_id.clone().filter(|_| games::busy(&g));
    s.set_launching_id(launching.unwrap_or_default().into());
    s.set_notice(
        g.last_import
            .as_ref()
            .map(|i| games::notice(i, &tr))
            .unwrap_or_default()
            .into(),
    );
    s.set_error(g.error.clone().unwrap_or_default().into());
    let controllers: Vec<ControllerData> = g
        .controllers
        .iter()
        .map(|c| ControllerData {
            mac: c.mac.clone().into(),
            name: c.name.clone().into(),
            paired: c.paired,
            connected: c.connected,
        })
        .collect();
    s.set_controllers(ModelRc::from(Rc::new(VecModel::from(controllers))));
    s.set_scanning(g.scanning);
    s.set_pairing_mac(g.pairing.clone().unwrap_or_default().into());
    s.set_controller_error(g.controller_error.clone().unwrap_or_default().into());

    let before = LAST_RUN.replace(g.run);
    if games::came_back(before, g.run) {
        if ui.get_page() != Page::Games {
            crate::choose(ui, Page::Games);
        }
        let at = g
            .game_id
            .as_ref()
            .and_then(|id| g.library.iter().position(|x| x.id == *id));
        let f = ui.global::<Focus>();
        if let Some(i) = at
            && !s.get_pairing_open()
        {
            f.set_main_index((HEADER + i) as i32);
            f.set_zone(FocusZone::Main);
        }
    }
}

pub fn set_texts(ui: &AppWindow, tr: impl Fn(&str, &str) -> String) {
    let t = ui.global::<GamesText>();
    t.set_title(tr("games.title", "Games").into());
    t.set_loading(tr("games.loading", "Loading games…").into());
    t.set_import(tr("games.import", "Import ROMs").into());
    t.set_importing(tr("games.importing", "Importing…").into());
    t.set_pair_controller(tr("games.pairController", "Pair controller").into());
    t.set_rescan(tr("games.rescan", "Rescan").into());
    t.set_empty_title(tr("games.emptyTitle", "No games yet").into());
    t.set_empty_roms(
        tr("games.emptyRoms", "Place legally obtained ROMs in {{dir}}.")
            .replace("{{dir}}", DEFAULT_ROM_DIRECTORY)
            .into(),
    );
    t.set_empty_import(
        tr(
            "games.emptyImport",
            "Import ROMs to create playlists and download cover art.",
        )
        .into(),
    );
    t.set_open_retroarch(tr("games.openRetroArch", "Open RetroArch").into());
    t.set_retroarch_dirs(
        tr(
            "games.retroArchDirs",
            "Playlists and thumbnails are read automatically from ~/.config/retroarch.",
        )
        .into(),
    );
    t.set_launching(tr("games.launching", "Starting RetroArch…").into());
    t.set_controllers_title(tr("games.controllersTitle", "Bluetooth controllers").into());
    t.set_controllers_hint(
        tr(
            "games.controllersHint",
            "Put your controller in pairing mode, then scan for nearby devices.",
        )
        .into(),
    );
    t.set_no_controllers(tr("games.noControllers", "No controllers found").into());
    t.set_scan(tr("games.scan", "Scan").into());
    t.set_scanning(tr("games.scanning", "Scanning…").into());
    t.set_close(tr("games.close", "Close").into());
    t.set_pair(tr("games.pair", "Pair").into());
    t.set_pairing(tr("games.pairing", "Pairing…").into());
    t.set_paired(tr("games.paired", "Paired").into());
    t.set_connected(tr("games.connected", "Connected").into());
    let n = ui.global::<GamesState>().get_games().row_count();
    t.set_count(games::count(n, &tr).into());
}

pub fn targets(ui: &AppWindow) -> usize {
    let s = ui.global::<GamesState>();
    if s.get_pairing_open() {
        return 2 + s.get_controllers().row_count();
    }
    match s.get_games().row_count() {
        0 if s.get_scanned() => HEADER + 2,
        n => HEADER + n,
    }
}

pub fn activate(ui: &AppWindow, i: usize) {
    let s = ui.global::<GamesState>();
    if s.get_pairing_open() {
        match i {
            0 => s.invoke_scan(),
            1 => s.invoke_close_pairing(),
            n => {
                if let Some(c) = s.get_controllers().row_data(n - 2)
                    && !c.paired
                {
                    s.invoke_pair(c.mac);
                }
            }
        }
        return;
    }
    match i {
        0 => s.invoke_import(),
        1 => s.invoke_open_pairing(),
        2 => s.invoke_rescan(),
        n => {
            let games = s.get_games();
            match games.row_data(n - HEADER) {
                Some(g) => s.invoke_launch(g.id),
                None if games.row_count() == 0 && n == HEADER => s.invoke_import(),
                None if games.row_count() == 0 => s.invoke_open_retroarch(),
                None => {}
            }
        }
    }
}

pub fn wire(ui: &AppWindow, client: &Client) {
    let s = ui.global::<GamesState>();
    let act = |action: Action| {
        let c = client.clone();
        move || c.act(action.clone())
    };
    s.on_import(act(Action::GamesImport));
    s.on_rescan(act(Action::GamesRefresh));
    s.on_open_retroarch(act(Action::GamesOpenRetroArch));
    s.on_scan(act(Action::ControllersScan));
    s.on_launch({
        let c = client.clone();
        move |id| c.act(Action::GamesLaunch { id: id.into() })
    });
    s.on_pair({
        let c = client.clone();
        move |mac| c.act(Action::ControllerPair { mac: mac.into() })
    });
    let weak = ui.as_weak();
    s.on_open_pairing({
        let (c, weak) = (client.clone(), weak.clone());
        move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.global::<GamesState>().set_pairing_open(true);
            ui.global::<Focus>().set_main_index(0);
            c.act(Action::ControllersList);
        }
    });
    s.on_close_pairing(move || {
        let Some(ui) = weak.upgrade() else { return };
        ui.global::<GamesState>().set_pairing_open(false);
        ui.global::<Focus>().set_main_index(1);
    });
}
