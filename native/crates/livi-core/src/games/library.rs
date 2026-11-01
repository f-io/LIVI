// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use livi_core_proto::config::GamesConfig;
use livi_core_proto::state::GameView;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const THUMBNAIL_MAX_BYTES: u64 = 8 * 1024 * 1024;
const THUMBNAIL_FOLDERS: [&str; 3] = ["Named_Boxarts", "Named_Titles", "Named_Snaps"];
const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub system: String,
    pub rom: String,
    pub core: Option<String>,
    pub thumbnail: Option<PathBuf>,
}

impl Game {
    pub fn view(&self) -> GameView {
        GameView {
            id: self.id.clone(),
            title: self.title.clone(),
            system: self.system.clone(),
            thumbnail: self
                .thumbnail
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub roms: PathBuf,
    pub playlists: PathBuf,
    pub thumbnails: PathBuf,
    pub cores: Vec<PathBuf>,
}

impl Dirs {
    pub fn of(cfg: &GamesConfig) -> Self {
        let mut cores = vec![expand(&cfg.cores_directory)];
        cores.extend(
            [
                "/usr/lib/aarch64-linux-gnu/libretro",
                "/usr/lib/arm-linux-gnueabihf/libretro",
                "/usr/lib/x86_64-linux-gnu/libretro",
                "/usr/lib/libretro",
            ]
            .map(PathBuf::from),
        );
        Self {
            roms: expand(&cfg.rom_directory),
            playlists: expand(&cfg.playlist_directory),
            thumbnails: expand(&cfg.thumbnail_directory),
            cores,
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

pub fn expand(dir: &str) -> PathBuf {
    let value = dir.trim();
    if value == "~" {
        return home();
    }
    match value.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(value),
    }
}

pub fn thumbnail_name(label: &str) -> String {
    label
        .chars()
        .map(|c| if "&*/:`<>?\\|".contains(c) { '_' } else { c })
        .collect()
}

pub fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&path, out),
            Ok(t) if t.is_file() => out.push(path),
            _ => {}
        }
    }
}

pub fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .unwrap_or_default()
}

fn playlists(dir: &Path) -> Vec<PathBuf> {
    if dir.as_os_str().is_empty() {
        return Vec::new();
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    files.retain(|p| extension(p) == "lpl");
    files.sort();
    files
}

fn thumbnail(dir: &Path, system: &str, label: &str) -> Option<PathBuf> {
    if dir.as_os_str().is_empty() {
        return None;
    }
    let mut names = vec![label.to_string()];
    let safe = thumbnail_name(label);
    if safe != label {
        names.push(safe);
    }
    THUMBNAIL_FOLDERS
        .iter()
        .flat_map(|folder| {
            names.iter().flat_map(move |name| {
                IMAGE_EXTENSIONS
                    .iter()
                    .map(move |ext| dir.join(system).join(folder).join(format!("{name}.{ext}")))
            })
        })
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= THUMBNAIL_MAX_BYTES))
}

pub fn game_id(rom: &str, core: Option<&str>) -> String {
    let digest = Sha256::digest(format!("{rom}\0{}", core.unwrap_or_default()));
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn text<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    item.get(key).and_then(Value::as_str)
}

pub fn scan(dirs: &Dirs) -> Vec<Game> {
    let mut games = Vec::new();
    let mut seen = HashSet::new();
    for path in playlists(&dirs.playlists) {
        let parsed = fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()));
        let playlist = match parsed {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[games] skipped invalid playlist {}: {e}", path.display());
                continue;
            }
        };
        let Some(items) = playlist.get("items").and_then(Value::as_array) else {
            continue;
        };
        let playlist_system = stem(&path);
        for item in items {
            let Some(rom) = text(item, "path").filter(|p| !p.trim().is_empty()) else {
                continue;
            };
            let title = text(item, "label")
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map_or_else(|| stem(Path::new(rom)), str::to_string);
            let system = text(item, "db_name")
                .filter(|d| !d.is_empty())
                .map_or_else(|| playlist_system.clone(), |d| stem(Path::new(d)));
            let core = text(item, "core_path")
                .filter(|c| !c.is_empty() && *c != "DETECT")
                .map(str::to_string);
            let id = game_id(rom, core.as_deref());
            if !seen.insert(id.clone()) {
                continue;
            }
            games.push(Game {
                id,
                thumbnail: thumbnail(&dirs.thumbnails, &system, &title),
                title,
                system,
                rom: rom.to_string(),
                core,
            });
        }
    }
    games.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.title.cmp(&b.title))
    });
    games
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::json;

    use super::*;
    use crate::config_file::tests::TempDir;

    pub fn dirs(root: &Path) -> Dirs {
        Dirs {
            roms: root.join("roms"),
            playlists: root.join("playlists"),
            thumbnails: root.join("thumbnails"),
            cores: vec![root.join("cores")],
        }
    }

    #[test]
    fn retroarch_paths_expand_from_the_home_directory() {
        assert_eq!(expand("~/Games/roms"), home().join("Games/roms"));
        assert_eq!(expand("~"), home());
        assert_eq!(expand("/srv/roms"), PathBuf::from("/srv/roms"));
    }

    #[test]
    fn playlists_are_read_with_their_box_art() {
        let tmp = TempDir::new();
        let d = dirs(&tmp.0);
        let system = "Nintendo - Nintendo Entertainment System";
        let art = d.thumbnails.join(system).join("Named_Boxarts");
        fs::create_dir_all(&art).unwrap();
        fs::create_dir_all(&d.playlists).unwrap();
        fs::write(art.join("Super Mario Bros..png"), "image").unwrap();
        let playlist = json!({ "version": "1.0", "items": [{
            "path": "/roms/mario.nes",
            "label": "Super Mario Bros.",
            "core_path": "/cores/nestopia_libretro.so",
            "db_name": format!("{system}.lpl"),
        }]});
        fs::write(
            d.playlists.join(format!("{system}.lpl")),
            playlist.to_string(),
        )
        .unwrap();

        let games = scan(&d);
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.title, "Super Mario Bros.");
        assert_eq!(g.system, system);
        assert_eq!(g.rom, "/roms/mario.nes");
        assert_eq!(g.core.as_deref(), Some("/cores/nestopia_libretro.so"));
        assert_eq!(
            g.thumbnail.as_deref(),
            Some(art.join("Super Mario Bros..png").as_path())
        );
        assert_eq!(g.id.len(), 24);
        assert!(g.view().thumbnail.is_some());
    }

    #[test]
    fn broken_playlists_are_skipped_and_duplicates_kept_once() {
        let tmp = TempDir::new();
        let mut d = dirs(&tmp.0);
        d.thumbnails = PathBuf::new();
        fs::create_dir_all(d.playlists.join("nested")).unwrap();
        fs::write(d.playlists.join("broken.lpl"), "{").unwrap();
        let item = json!({ "path": "/roms/game.zip", "label": "Game", "core_path": "DETECT" });
        fs::write(
            d.playlists.join("nested/Arcade.lpl"),
            json!({ "items": [item, item] }).to_string(),
        )
        .unwrap();

        let games = scan(&d);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Game");
        assert_eq!(games[0].system, "Arcade");
        assert_eq!(games[0].core, None);
        assert_eq!(games[0].thumbnail, None);
    }

    #[test]
    fn a_label_falls_back_to_the_file_name_and_thumbnails_to_the_safe_name() {
        let tmp = TempDir::new();
        let d = dirs(&tmp.0);
        fs::create_dir_all(&d.playlists).unwrap();
        let snaps = d.thumbnails.join("DOOM/Named_Snaps");
        fs::create_dir_all(&snaps).unwrap();
        fs::write(snaps.join("A_B.jpg"), "image").unwrap();
        let items = json!({ "items": [
            { "path": "/roms/doom/b.wad", "label": "A/B" },
            { "path": "/roms/doom/Another One.wad" },
            { "label": "no path" },
        ]});
        fs::write(d.playlists.join("DOOM.lpl"), items.to_string()).unwrap();
        let games = scan(&d);
        let titles: Vec<_> = games.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["A/B", "Another One"]);
        assert_eq!(games[0].thumbnail, Some(snaps.join("A_B.jpg")));
    }

    #[test]
    fn box_art_names_lose_the_characters_libretro_replaces() {
        assert_eq!(
            thumbnail_name("Ys I & II: <Eternal>?"),
            "Ys I _ II_ _Eternal__"
        );
    }
}
