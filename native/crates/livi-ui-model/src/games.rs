// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::{GameImport, GameRun, Games};
use serde_json::Value;

pub fn read(state: &Value) -> Games {
    serde_json::from_value(state["games"].clone()).unwrap_or_default()
}

pub fn busy(g: &Games) -> bool {
    g.run != GameRun::Idle
}

fn fill(text: String, vars: &[(&str, String)]) -> String {
    vars.iter().fold(text, |s, (name, value)| {
        s.replace(&format!("{{{{{name}}}}}"), value)
    })
}

pub fn count(n: usize, tr: &impl Fn(&str, &str) -> String) -> String {
    if n == 1 {
        return tr("games.countOne", "1 game");
    }
    fill(
        tr("games.count", "{{count}} games"),
        &[("count", n.to_string())],
    )
}

pub fn notice(import: &GameImport, tr: &impl Fn(&str, &str) -> String) -> String {
    let done = fill(
        tr(
            "games.imported",
            "Imported {{games}} games into {{playlists}} playlists; downloaded {{thumbnails}} thumbnails.",
        ),
        &[
            ("games", import.games.to_string()),
            ("playlists", import.playlists.to_string()),
            ("thumbnails", import.thumbnails_downloaded.to_string()),
        ],
    );
    if import.missing_cores.is_empty() {
        return done;
    }
    let cores = fill(
        tr("games.missingCores", "Missing cores: {{cores}}."),
        &[("cores", import.missing_cores.join(", "))],
    );
    format!("{done} {cores}")
}

// The run went from a game back to idle: LIVI shows the Games page again.
pub fn came_back(before: GameRun, now: GameRun) -> bool {
    before != GameRun::Idle && now == GameRun::Idle
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn en(key: &str, text: &str) -> String {
        crate::i18n::Strings::new("en").t(key, text)
    }

    #[test]
    fn the_count_reads_naturally() {
        assert_eq!(count(1, &en), "1 game");
        assert_eq!(count(0, &en), "0 games");
        assert_eq!(count(12, &en), "12 games");
    }

    #[test]
    fn an_import_says_what_it_did_and_which_cores_are_missing() {
        let mut import = GameImport {
            games: 3,
            playlists: 2,
            thumbnails_downloaded: 1,
            thumbnails_missing: 2,
            missing_cores: vec![],
        };
        assert_eq!(
            notice(&import, &en),
            "Imported 3 games into 2 playlists; downloaded 1 thumbnails."
        );
        import.missing_cores = vec!["DOOM".into(), "Nintendo - Game Boy".into()];
        assert!(notice(&import, &en).ends_with(" Missing cores: DOOM, Nintendo - Game Boy."));
    }

    #[test]
    fn the_state_reads_with_defaults() {
        let g = read(&json!({ "games": { "library": [
            { "id": "a", "title": "Tetris", "system": "Nintendo - Game Boy" }
        ], "run": "running" } }));
        assert_eq!(g.library[0].title, "Tetris");
        assert!(busy(&g));
        assert!(!busy(&read(&json!({}))));
        assert!(came_back(GameRun::Running, GameRun::Idle));
        assert!(came_back(GameRun::Launching, GameRun::Idle));
        assert!(!came_back(GameRun::Idle, GameRun::Idle));
        assert!(!came_back(GameRun::Idle, GameRun::Running));
    }
}
