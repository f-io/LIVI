// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::{HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use livi_core_proto::state::GameImport;
use serde_json::{Value, json};

use super::library::{Dirs, THUMBNAIL_MAX_BYTES, extension, stem, thumbnail_name, walk};

const DOWNLOADS_AT_ONCE: usize = 6;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(8);
const ARCHIVES: [&str; 2] = ["zip", "7z"];
const MULTI_DISC: [&str; 4] = ["cue", "chd", "pbp", "ccd"];

pub struct System {
    pub id: &'static str,
    pub folder: &'static str,
    pub playlist: &'static str,
    pub thumbnail_repo: &'static str,
    aliases: &'static [&'static str],
    extensions: &'static [&'static str],
    cores: &'static [(&'static str, &'static str)],
}

pub const SYSTEMS: [System; 9] = [
    System {
        id: "doom",
        folder: "doom",
        playlist: "DOOM",
        thumbnail_repo: "DOOM",
        aliases: &["doom"],
        extensions: &["wad"],
        cores: &[("prboom_libretro.so", "PrBoom")],
    },
    System {
        id: "gba",
        folder: "gba",
        playlist: "Nintendo - Game Boy Advance",
        thumbnail_repo: "Nintendo_-_Game_Boy_Advance",
        aliases: &["gba", "gameboyadvance"],
        extensions: &["gba"],
        cores: &[("mgba_libretro.so", "mGBA")],
    },
    System {
        id: "gbc",
        folder: "gameboy-color",
        playlist: "Nintendo - Game Boy Color",
        thumbnail_repo: "Nintendo_-_Game_Boy_Color",
        aliases: &["gbc", "gameboycolor"],
        extensions: &["gbc"],
        cores: &[
            ("gambatte_libretro.so", "Gambatte"),
            ("mgba_libretro.so", "mGBA"),
        ],
    },
    System {
        id: "gb",
        folder: "gameboy",
        playlist: "Nintendo - Game Boy",
        thumbnail_repo: "Nintendo_-_Game_Boy",
        aliases: &["gb", "gameboy"],
        extensions: &["gb"],
        cores: &[
            ("gambatte_libretro.so", "Gambatte"),
            ("mgba_libretro.so", "mGBA"),
        ],
    },
    System {
        id: "nds",
        folder: "nds",
        playlist: "Nintendo - Nintendo DS",
        thumbnail_repo: "Nintendo_-_Nintendo_DS",
        aliases: &["nds", "nintendods"],
        extensions: &["nds"],
        cores: &[("desmume_libretro.so", "DeSmuME")],
    },
    System {
        id: "nes",
        folder: "nes",
        playlist: "Nintendo - Nintendo Entertainment System",
        thumbnail_repo: "Nintendo_-_Nintendo_Entertainment_System",
        aliases: &["nes", "nintendoentertainmentsystem"],
        extensions: &["nes"],
        cores: &[("nestopia_libretro.so", "Nestopia UE")],
    },
    System {
        id: "snes",
        folder: "snes",
        playlist: "Nintendo - Super Nintendo Entertainment System",
        thumbnail_repo: "Nintendo_-_Super_Nintendo_Entertainment_System",
        aliases: &["snes", "supernintendo"],
        extensions: &["sfc", "smc"],
        cores: &[("snes9x_libretro.so", "Snes9x")],
    },
    System {
        id: "genesis",
        folder: "genesis",
        playlist: "Sega - Mega Drive - Genesis",
        thumbnail_repo: "Sega_-_Mega_Drive_-_Genesis",
        aliases: &["genesis", "megadrive"],
        extensions: &["md", "gen", "smd"],
        cores: &[("genesis_plus_gx_libretro.so", "Genesis Plus GX")],
    },
    System {
        id: "mastersystem",
        folder: "master-system",
        playlist: "Sega - Master System - Mark III",
        thumbnail_repo: "Sega_-_Master_System_-_Mark_III",
        aliases: &["mastersystem", "sms"],
        extensions: &["sms", "sg", "gg"],
        cores: &[("genesis_plus_gx_libretro.so", "Genesis Plus GX")],
    },
];

#[derive(Debug, Clone)]
pub struct Rom {
    pub path: PathBuf,
    pub label: String,
    pub system: usize,
    pub core: Option<(PathBuf, &'static str)>,
}

fn folded(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn system_for(path: &Path, root: &Path) -> Option<usize> {
    let ext = extension(path);
    let relative = path.strip_prefix(root).unwrap_or(path);
    let folders: Vec<String> = relative
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .map(|c| folded(&c.as_os_str().to_string_lossy()))
        .collect();
    let by_folder = SYSTEMS
        .iter()
        .position(|s| folders.iter().any(|f| s.aliases.contains(&f.as_str())));
    if let Some(i) = by_folder
        && (SYSTEMS[i].extensions.contains(&ext.as_str()) || ARCHIVES.contains(&ext.as_str()))
    {
        return Some(i);
    }
    SYSTEMS
        .iter()
        .position(|s| s.extensions.contains(&ext.as_str()))
}

fn find_core(system: &System, roots: &[PathBuf]) -> Option<(PathBuf, &'static str)> {
    system.cores.iter().find_map(|(file, name)| {
        roots
            .iter()
            .map(|r| r.join(file))
            .find(|p| p.is_file())
            .map(|p| (p, *name))
    })
}

pub fn collect(dirs: &Dirs) -> io::Result<Vec<Rom>> {
    fs::create_dir_all(&dirs.roms)?;
    for system in &SYSTEMS {
        fs::create_dir_all(dirs.roms.join(system.folder))?;
    }
    let mut files = Vec::new();
    walk(&dirs.roms, &mut files);
    files.sort();
    let multi_disc: HashSet<PathBuf> = files
        .iter()
        .filter(|p| extension(p) == "m3u")
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    let mut cores: HashMap<&'static str, Option<(PathBuf, &'static str)>> = HashMap::new();
    let mut roms = Vec::new();
    for path in files {
        let disc = MULTI_DISC.contains(&extension(&path).as_str());
        if disc && path.parent().is_some_and(|p| multi_disc.contains(p)) {
            continue;
        }
        let Some(system) = system_for(&path, &dirs.roms) else {
            continue;
        };
        let core = cores
            .entry(SYSTEMS[system].id)
            .or_insert_with(|| find_core(&SYSTEMS[system], &dirs.cores))
            .clone();
        roms.push(Rom {
            label: stem(&path),
            path,
            system,
            core,
        });
    }
    Ok(roms)
}

fn write_atomic(path: &Path, data: &str) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

pub fn write_playlist(dirs: &Dirs, system: &System, roms: &[&Rom]) -> io::Result<()> {
    fs::create_dir_all(&dirs.playlists)?;
    let path = dirs.playlists.join(format!("{}.lpl", system.playlist));
    let previous: Value = fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    let mut items: Vec<Value> = previous
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| {
            item.get("path")
                .and_then(Value::as_str)
                .is_none_or(|p| !Path::new(p).starts_with(&dirs.roms))
        })
        .cloned()
        .collect();
    let db_name = format!("{}.lpl", system.playlist);
    items.extend(roms.iter().map(|rom| {
        let (core_path, core_name) = match &rom.core {
            Some((path, name)) => (path.to_string_lossy().into_owned(), name.to_string()),
            None => ("DETECT".to_string(), "DETECT".to_string()),
        };
        json!({
            "path": rom.path.to_string_lossy(),
            "label": rom.label,
            "core_path": core_path,
            "core_name": core_name,
            "crc32": "DETECT",
            "db_name": db_name,
        })
    }));
    let text = serde_json::to_string_pretty(&json!({ "version": "1.5", "items": items }))
        .map_err(io::Error::other)?;
    write_atomic(&path, &text)
}

pub fn encode_uri_component(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn thumbnail_url(system: &System, label: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/libretro-thumbnails/{}/master/Named_Boxarts/{}",
        system.thumbnail_repo,
        encode_uri_component(&format!("{}.png", thumbnail_name(label)))
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fetched {
    Existing,
    Downloaded,
    Missing,
}

async fn download<F, Fut>(thumbnails: PathBuf, rom: Rom, fetch: F) -> Fetched
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Option<Vec<u8>>>,
{
    let system = &SYSTEMS[rom.system];
    let folder = thumbnails.join(system.playlist).join("Named_Boxarts");
    let dest = folder.join(format!("{}.png", thumbnail_name(&rom.label)));
    if dest.is_file() {
        return Fetched::Existing;
    }
    let Some(bytes) = fetch(thumbnail_url(system, &rom.label)).await else {
        return Fetched::Missing;
    };
    if bytes.is_empty() || bytes.len() as u64 > THUMBNAIL_MAX_BYTES {
        return Fetched::Missing;
    }
    let written = fs::create_dir_all(&folder).and_then(|()| fs::write(&dest, bytes));
    match written {
        Ok(()) => Fetched::Downloaded,
        Err(e) => {
            eprintln!("[games] box art {} not saved: {e}", dest.display());
            Fetched::Missing
        }
    }
}

pub async fn http_fetch(url: String) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .ok()?;
    let response = client.get(&url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().await.ok().map(|b| b.to_vec())
}

pub async fn import<F, Fut>(dirs: Dirs, fetch: F) -> Result<GameImport, String>
where
    F: Fn(String) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Option<Vec<u8>>> + Send,
{
    let scan_dirs = dirs.clone();
    let roms = tokio::task::spawn_blocking(move || -> io::Result<(Vec<Rom>, u32)> {
        let roms = collect(&scan_dirs)?;
        let mut playlists = 0;
        for (i, system) in SYSTEMS.iter().enumerate() {
            let of_system: Vec<&Rom> = roms.iter().filter(|r| r.system == i).collect();
            if of_system.is_empty() {
                continue;
            }
            write_playlist(&scan_dirs, system, &of_system)?;
            playlists += 1;
        }
        Ok((roms, playlists))
    })
    .await
    .map_err(|e| e.to_string())?;
    let (roms, playlists) = roms.map_err(|e| format!("ROM import failed: {e}"))?;

    let downloaded = Arc::new(AtomicU32::new(0));
    let missing = Arc::new(AtomicU32::new(0));
    let gate = Arc::new(tokio::sync::Semaphore::new(DOWNLOADS_AT_ONCE));
    let mut jobs = tokio::task::JoinSet::new();
    for rom in roms.iter().cloned() {
        let (fetch, gate) = (fetch.clone(), gate.clone());
        let (downloaded, missing) = (downloaded.clone(), missing.clone());
        let thumbnails = dirs.thumbnails.clone();
        jobs.spawn(async move {
            let Ok(_slot) = gate.acquire().await else {
                return;
            };
            match download(thumbnails, rom, fetch).await {
                Fetched::Downloaded => downloaded.fetch_add(1, Ordering::Relaxed),
                Fetched::Missing => missing.fetch_add(1, Ordering::Relaxed),
                Fetched::Existing => 0,
            };
        });
    }
    while jobs.join_next().await.is_some() {}

    let mut seen = HashSet::new();
    let missing_cores = roms
        .iter()
        .filter(|r| r.core.is_none() && seen.insert(r.system))
        .map(|r| SYSTEMS[r.system].playlist.to_string())
        .collect();
    Ok(GameImport {
        games: roms.len() as u32,
        playlists,
        thumbnails_downloaded: downloaded.load(Ordering::Relaxed),
        thumbnails_missing: missing.load(Ordering::Relaxed),
        missing_cores,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;
    use crate::games::library::{self, tests::dirs};

    fn fixture(tmp: &TempDir) -> Dirs {
        let d = dirs(&tmp.0);
        for (folder, file) in [
            ("doom", "DOOM (Freedoom).wad"),
            ("gameboy", "Tetris (World).gb"),
            ("gba", "Advance Wars (USA).gba"),
        ] {
            fs::create_dir_all(d.roms.join(folder)).unwrap();
            fs::write(d.roms.join(folder).join(file), "rom").unwrap();
        }
        d
    }

    async fn three_bytes(_url: String) -> Option<Vec<u8>> {
        Some(vec![1, 2, 3])
    }

    #[tokio::test]
    async fn an_import_writes_playlists_and_downloads_box_art() {
        let tmp = TempDir::new();
        let d = fixture(&tmp);
        let result = import(d.clone(), three_bytes).await.unwrap();
        assert_eq!(result.games, 3);
        assert_eq!(result.playlists, 3);
        assert_eq!(result.thumbnails_downloaded, 3);
        assert_eq!(result.thumbnails_missing, 0);
        for folder in [
            "doom",
            "gameboy",
            "gameboy-color",
            "gba",
            "nds",
            "nes",
            "snes",
            "genesis",
            "master-system",
        ] {
            assert!(d.roms.join(folder).is_dir(), "{folder}");
        }
        assert_eq!(
            result.missing_cores,
            ["DOOM", "Nintendo - Game Boy", "Nintendo - Game Boy Advance"]
        );
        assert!(
            d.thumbnails
                .join("Nintendo - Game Boy/Named_Boxarts/Tetris (World).png")
                .is_file()
        );

        let again = import(d.clone(), three_bytes).await.unwrap();
        assert_eq!(again.thumbnails_downloaded, 0);
        let games = library::scan(&d);
        let titles: Vec<_> = games.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Advance Wars (USA)", "DOOM (Freedoom)", "Tetris (World)"]
        );
        assert!(games.iter().all(|g| g.thumbnail.is_some()));
    }

    #[tokio::test]
    async fn installed_cores_are_written_and_other_entries_kept() {
        let tmp = TempDir::new();
        let d = fixture(&tmp);
        fs::create_dir_all(&d.cores[0]).unwrap();
        fs::write(d.cores[0].join("mgba_libretro.so"), "core").unwrap();
        fs::create_dir_all(&d.playlists).unwrap();
        let foreign = json!({ "items": [
            { "path": "/elsewhere/Pokemon.gba", "label": "Pokemon" },
            { "path": d.roms.join("gba/Old.gba").to_string_lossy(), "label": "Old" },
        ]});
        fs::write(
            d.playlists.join("Nintendo - Game Boy Advance.lpl"),
            foreign.to_string(),
        )
        .unwrap();

        async fn nothing(_url: String) -> Option<Vec<u8>> {
            None
        }
        let result = import(d.clone(), nothing).await.unwrap();
        assert_eq!(result.thumbnails_missing, 3);
        assert_eq!(result.missing_cores, ["DOOM"]);
        let text = fs::read_to_string(d.playlists.join("Nintendo - Game Boy Advance.lpl")).unwrap();
        let lpl: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(lpl["version"], "1.5");
        let items = lpl["items"].as_array().unwrap();
        let labels: Vec<_> = items.iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["Pokemon", "Advance Wars (USA)"]);
        assert_eq!(
            items[1]["core_path"],
            d.cores[0]
                .join("mgba_libretro.so")
                .to_string_lossy()
                .as_ref()
        );
        assert_eq!(items[1]["core_name"], "mGBA");
        assert_eq!(items[1]["db_name"], "Nintendo - Game Boy Advance.lpl");
        let gb: Value = serde_json::from_str(
            &fs::read_to_string(d.playlists.join("Nintendo - Game Boy.lpl")).unwrap(),
        )
        .unwrap();
        assert_eq!(gb["items"][0]["core_name"], "mGBA");
    }

    #[test]
    fn systems_come_from_the_folder_or_the_extension() {
        let root = Path::new("/roms");
        let id = |p: &str| system_for(Path::new(p), root).map(|i| SYSTEMS[i].id);
        assert_eq!(id("/roms/gameboy-color/zelda.gbc"), Some("gbc"));
        assert_eq!(id("/roms/Game Boy Color/pack.zip"), Some("gbc"));
        assert_eq!(id("/roms/gameboy/tetris.zip"), Some("gb"));
        assert_eq!(id("/roms/Mega_Drive/sonic.7z"), Some("genesis"));
        assert_eq!(id("/roms/misc/sonic.md"), Some("genesis"));
        assert_eq!(id("/roms/snes/mario.smc"), Some("snes"));
        assert_eq!(id("/roms/nes/notes.txt"), None);
        assert_eq!(id("/roms/misc/archive.zip"), None);
        assert_eq!(id("/roms/doom/freedoom2.wad"), Some("doom"));
    }

    #[test]
    fn multi_disc_folders_skip_their_discs() {
        let tmp = TempDir::new();
        let d = dirs(&tmp.0);
        let ff = d.roms.join("psx/Final Fantasy VII");
        fs::create_dir_all(&ff).unwrap();
        fs::write(ff.join("FF7.m3u"), "disc1.cue\n").unwrap();
        fs::write(ff.join("disc1.cue"), "").unwrap();
        fs::write(ff.join("disc1.gb"), "").unwrap();
        let roms = collect(&d).unwrap();
        let names: Vec<_> = roms.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(names, ["disc1"]);
    }

    #[test]
    fn box_art_urls_follow_the_libretro_repositories() {
        assert_eq!(
            thumbnail_url(&SYSTEMS[3], "Tetris (World)"),
            "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Game_Boy/master/Named_Boxarts/Tetris%20(World).png"
        );
        assert_eq!(encode_uri_component("A&B: é"), "A%26B%3A%20%C3%A9");
    }
}
