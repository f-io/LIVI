// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::process::Stdio;

use livi_core_proto::state::{Screen, State};
use livi_media::compositor::Ui;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use super::library::Game;

pub fn game_args(game: &Game) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(core) = &game.core {
        args.extend(["-L".to_string(), core.clone()]);
    }
    args.push(game.rom.clone());
    args
}

pub fn menu_args() -> Vec<String> {
    vec!["--menu".into()]
}

pub fn args(size: Option<(u32, u32)>, rest: Vec<String>) -> Vec<String> {
    let mut out = vec!["--fullscreen".to_string()];
    // Some Wayland compositors ignore RetroArch's own fullscreen request, a window
    // of the screen's size still fills it without borders.
    if let Some((w, h)) = size.filter(|(w, h)| *w > 0 && *h > 0) {
        out.push(format!("--size={w}x{h}"));
    }
    out.extend(rest);
    out
}

fn mode_size(mode: &str) -> Option<(u32, u32)> {
    let size = mode.split('@').next()?;
    let (w, h) = size.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

pub fn main_size(state: &State) -> Option<(u32, u32)> {
    state
        .system
        .displays
        .iter()
        .find(|d| d.screen == Some(Screen::Main))
        .and_then(|d| d.current.as_deref())
        .and_then(mode_size)
        .or_else(|| {
            let c = &state.config;
            (c.main_screen_width > 0 && c.main_screen_height > 0)
                .then_some((c.main_screen_width, c.main_screen_height))
        })
}

fn log_lines(from: impl AsyncRead + Unpin + Send + 'static) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(from).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            println!("[retroarch] {line}");
        }
    });
}

pub fn spawn(exe: &str, args: &[String], ui: Option<&Ui>) -> std::io::Result<Child> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(ui) = ui {
        cmd.env("WAYLAND_DISPLAY", &ui.wayland_display)
            .env("XDG_SESSION_TYPE", "wayland")
            .env_remove("DISPLAY");
    }
    let mut child = cmd.spawn()?;
    if let Some(out) = child.stdout.take() {
        log_lines(out);
    }
    if let Some(err) = child.stderr.take() {
        log_lines(err);
    }
    Ok(child)
}

#[cfg(test)]
mod tests {
    use livi_core_proto::state::Display;

    use super::*;

    fn game(core: Option<&str>) -> Game {
        Game {
            id: "game-id".into(),
            title: "Game".into(),
            system: "System".into(),
            rom: "/roms/game.rom".into(),
            core: core.map(str::to_string),
            thumbnail: None,
        }
    }

    #[test]
    fn a_game_starts_fullscreen_at_the_screen_size_with_its_core() {
        assert_eq!(
            args(Some((800, 480)), game_args(&game(Some("/cores/core.so")))),
            [
                "--fullscreen",
                "--size=800x480",
                "-L",
                "/cores/core.so",
                "/roms/game.rom"
            ]
        );
        assert_eq!(
            args(None, game_args(&game(None))),
            ["--fullscreen", "/roms/game.rom"]
        );
    }

    #[test]
    fn the_menu_opens_fullscreen_for_library_setup() {
        assert_eq!(
            args(Some((800, 480)), menu_args()),
            ["--fullscreen", "--size=800x480", "--menu"]
        );
        assert_eq!(
            args(Some((0, 480)), menu_args()),
            ["--fullscreen", "--menu"]
        );
    }

    #[test]
    fn the_size_is_the_main_display_mode_or_the_configured_window() {
        let mut state = crate::games::tests::state();
        assert_eq!(main_size(&state), Some((1280, 720)));
        state.system.displays.push(Display {
            screen: Some(Screen::Main),
            current: Some("1920x1080@60".into()),
            ..Default::default()
        });
        assert_eq!(main_size(&state), Some((1920, 1080)));
    }
}
