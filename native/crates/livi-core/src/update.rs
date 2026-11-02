// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::state::{Release, Update, UpdatePhase};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::server::Core;

const NIGHTLY_TAG: &str = "nightly";
const USER_AGENT: &str = "LIVI-updater";
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
const PACKAGE: &str = if cfg!(target_os = "macos") {
    ".dmg"
} else if cfg!(target_os = "android") {
    ".apk"
} else {
    ".appimage"
};

pub type Install = dyn Fn(&Path) -> Result<(), String> + Send + Sync;

#[derive(Clone)]
pub struct Installer {
    pub downloads: PathBuf,
    pub install: Arc<Install>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAsk {
    Check,
    Download,
    Install,
    Abort,
}

fn feed_url(nightly: bool) -> String {
    if let Ok(feed) = std::env::var("UPDATE_FEED")
        && !feed.is_empty()
    {
        return feed;
    }
    let repo = std::env::var("UPDATE_REPO").ok().filter(|r| !r.is_empty());
    let base = format!(
        "https://api.github.com/repos/{}/releases",
        repo.as_deref().unwrap_or("f-io/LIVI")
    );
    if nightly {
        format!("{base}/tags/{NIGHTLY_TAG}")
    } else {
        format!("{base}/latest")
    }
}

fn run_number(title: &str) -> String {
    let Some(at) = title.find('#') else {
        return String::new();
    };
    title[at + 1..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

fn pick_asset(assets: &[Value], arch: &str, package: &str) -> Option<String> {
    let names: &[&str] = match arch {
        "x86_64" => &["x86_64", "amd64", "x64"],
        "aarch64" => &["arm64", "aarch64"],
        _ => return None,
    };
    assets.iter().find_map(|a| {
        let url = a.get("browser_download_url").and_then(Value::as_str);
        let name = a
            .get("name")
            .and_then(Value::as_str)
            .or(url)?
            .to_lowercase();
        let stem = name.strip_suffix(package)?;
        names
            .iter()
            .any(|n| {
                stem.strip_suffix(n)
                    .is_some_and(|rest| rest.ends_with(['-', '_', '.']))
            })
            .then(|| url.map(str::to_string))
            .flatten()
    })
}

fn release(feed: &Value) -> Release {
    let text = |key: &str| {
        feed.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let tag = feed
        .get("tag_name")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .or_else(|| feed.get("name").and_then(Value::as_str))
        .unwrap_or("");
    let version = tag.strip_prefix(['v', 'V']).unwrap_or(tag).to_string();
    let assets = feed
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let url = pick_asset(&assets, std::env::consts::ARCH, PACKAGE);
    Release {
        version,
        commit: text("target_commitish"),
        run: run_number(&text("name")),
        url,
    }
}

async fn check(client: &reqwest::Client, nightly: bool) -> Result<Release, String> {
    let feed = feed_url(nightly);
    let res = client.get(&feed).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("feed {}", res.status().as_u16()));
    }
    let body = res.bytes().await.map_err(|e| e.to_string())?;
    let json: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    Ok(release(&json))
}

fn publish(core: &Core, update: &Update) {
    let update = update.clone();
    core.hub.update(|s| s.update = update);
}

#[cfg(not(target_os = "macos"))]
async fn install(downloaded: &Path) -> Result<PathBuf, String> {
    let current = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .ok_or("LIVI does not run from an AppImage")?;
    let mut staged = current.clone().into_os_string();
    staged.push(".new");
    let staged = PathBuf::from(staged);
    tokio::fs::copy(downloaded, &staged)
        .await
        .map_err(|e| e.to_string())?;
    let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    tokio::fs::set_permissions(&staged, mode)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&staged, &current)
        .await
        .map_err(|e| e.to_string())?;
    Ok(current)
}

#[cfg(target_os = "macos")]
fn running_app() -> Option<PathBuf> {
    let resources = PathBuf::from(std::env::var_os("LIVI_RESOURCES")?);
    let app = resources.parent()?.parent()?;
    app.extension()
        .is_some_and(|e| e == "app")
        .then(|| app.to_path_buf())
}

#[cfg(target_os = "macos")]
const SWAP_APP: &str = r#"set -e
src="$1"; dst="$2"
rm -rf "$dst.new" "$dst.old"
ditto "$src" "$dst.new"
xattr -cr "$dst.new"
chown -R "$(stat -f %u:%g "$dst")" "$dst.new"
mv "$dst" "$dst.old"
mv "$dst.new" "$dst"
rm -rf "$dst.old""#;

#[cfg(target_os = "macos")]
fn sh_quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(target_os = "macos")]
fn swap_as_admin(src: &Path, dst: &Path) -> String {
    let line = format!(
        "sh -c {} sh {} {}",
        sh_quoted(SWAP_APP),
        sh_quoted(&src.display().to_string()),
        sh_quoted(&dst.display().to_string())
    );
    let quoted = line.replace('\\', "\\\\").replace('"', "\\\"");
    format!("do shell script \"{quoted}\" with administrator privileges")
}

#[cfg(target_os = "macos")]
async fn ran(cmd: &mut tokio::process::Command) -> Result<(), String> {
    let program = cmd.as_std().get_program().to_string_lossy().into_owned();
    let out = cmd.output().await.map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "{program}: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

#[cfg(target_os = "macos")]
async fn install(downloaded: &Path) -> Result<PathBuf, String> {
    use tokio::process::Command;
    let app = running_app().ok_or("LIVI does not run from an app bundle")?;
    let mount = std::env::temp_dir().join(format!("livi-update-{}", std::process::id()));
    ran(Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-mountpoint"])
        .arg(&mount)
        .arg(downloaded))
    .await?;
    let swapped = async {
        let mut entries = tokio::fs::read_dir(&mount)
            .await
            .map_err(|e| e.to_string())?;
        let mut new_app = None;
        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            if entry.path().extension().is_some_and(|e| e == "app") {
                new_app = Some(entry.path());
            }
        }
        let new_app = new_app.ok_or("the disk image holds no app")?;
        let mut swap = Command::new("sh");
        swap.args(["-c", SWAP_APP, "sh"]).arg(&new_app).arg(&app);
        if ran(&mut swap).await.is_ok() {
            return Ok(());
        }
        ran(Command::new("osascript")
            .arg("-e")
            .arg(swap_as_admin(&new_app, &app)))
        .await
    }
    .await;
    let _ = ran(Command::new("hdiutil")
        .arg("detach")
        .arg(&mount)
        .arg("-force"))
    .await;
    swapped.map(|()| app)
}

struct Download {
    stop: Option<oneshot::Sender<()>>,
}

async fn download(
    client: reqwest::Client,
    url: String,
    file: PathBuf,
    progress: mpsc::UnboundedSender<(f64, f64)>,
) -> Result<(), String> {
    let mut res = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("download {}", res.status().as_u16()));
    }
    let total = res.content_length().unwrap_or(0) as f64;
    let mut out = tokio::fs::File::create(&file)
        .await
        .map_err(|e| e.to_string())?;
    let mut received = 0.0;
    let mut told = Instant::now();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        out.write_all(&chunk).await.map_err(|e| e.to_string())?;
        received += chunk.len() as f64;
        if told.elapsed() >= PROGRESS_EVERY {
            told = Instant::now();
            let _ = progress.send((received, total));
        }
    }
    out.flush().await.map_err(|e| e.to_string())?;
    let _ = progress.send((received, total));
    Ok(())
}

pub async fn run(
    core: Arc<Core>,
    mut asks: mpsc::UnboundedReceiver<UpdateAsk>,
    installer: Option<Installer>,
) {
    let client = match crate::http::builder().user_agent(USER_AGENT).build() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[update] no http client: {e}");
            return;
        }
    };
    let downloads = match &installer {
        Some(i) => {
            let _ = tokio::fs::remove_dir_all(&i.downloads).await;
            if let Err(e) = tokio::fs::create_dir_all(&i.downloads).await {
                eprintln!("[update] {}: {e}", i.downloads.display());
            }
            i.downloads.clone()
        }
        None => std::env::temp_dir(),
    };
    let mut state = Update::default();
    let mut running: Option<Download> = None;
    let mut file: Option<PathBuf> = None;
    let (progress_tx, mut progress) = mpsc::unbounded_channel();
    let (done_tx, mut done) = mpsc::unbounded_channel::<Result<(), String>>();
    loop {
        tokio::select! {
            ask = asks.recv() => match ask {
                None => return,
                Some(UpdateAsk::Check) => {
                    state.checking = true;
                    publish(&core, &state);
                    let nightly = core.hub.config().update_nightly;
                    state.latest = match check(&client, nightly).await {
                        Ok(r) => Some(r),
                        Err(e) => {
                            eprintln!("[update] the {} feed did not answer: {e}",
                                if nightly { "nightly" } else { "release" });
                            None
                        }
                    };
                    state.checking = false;
                    state.checked = true;
                    publish(&core, &state);
                }
                Some(UpdateAsk::Download) => {
                    if !matches!(state.phase, UpdatePhase::Idle | UpdatePhase::Error) {
                        continue;
                    }
                    let Some(url) = state.latest.as_ref().and_then(|r| r.url.clone()) else {
                        state.phase = UpdatePhase::Error;
                        state.error = Some("No build for this machine".into());
                        publish(&core, &state);
                        continue;
                    };
                    let target =
                        downloads.join(format!("livi-update-{}{PACKAGE}", std::process::id()));
                    state = Update {
                        latest: state.latest.take(),
                        checked: true,
                        phase: UpdatePhase::Download,
                        ..Default::default()
                    };
                    publish(&core, &state);
                    let (stop, stopped) = oneshot::channel::<()>();
                    let (client, progress, done, path) =
                        (client.clone(), progress_tx.clone(), done_tx.clone(), target.clone());
                    tokio::spawn(async move {
                        let result = tokio::select! {
                            r = download(client, url, path, progress) => r,
                            _ = stopped => return,
                        };
                        let _ = done.send(result);
                    });
                    running = Some(Download { stop: Some(stop) });
                    file = Some(target);
                }
                Some(UpdateAsk::Abort) => {
                    if let Some(mut d) = running.take() {
                        drop(d.stop.take());
                    }
                    if let Some(f) = file.take() {
                        let _ = tokio::fs::remove_file(f).await;
                    }
                    state.phase = UpdatePhase::Error;
                    state.error = Some("Aborted".into());
                    publish(&core, &state);
                }
                Some(UpdateAsk::Install) => {
                    let Some(downloaded) = file.clone().filter(|_| state.phase == UpdatePhase::Ready) else {
                        continue;
                    };
                    state.phase = UpdatePhase::Installing;
                    publish(&core, &state);
                    let installed = match &installer {
                        Some(i) => {
                            let (install, file) = (i.install.clone(), downloaded.clone());
                            tokio::task::spawn_blocking(move || install(&file))
                                .await
                                .map_err(|e| e.to_string())
                                .and_then(|r| r)
                                .map(|()| None)
                        }
                        None => install(&downloaded).await.map(Some),
                    };
                    match installed {
                        Ok(app) => {
                            let _ = tokio::fs::remove_file(&downloaded).await;
                            state.phase = UpdatePhase::Relaunching;
                            publish(&core, &state);
                            if let Some(app) = app {
                                println!("[update] {} replaced, starting it", app.display());
                                core.relaunch(app);
                            }
                        }
                        Err(e) => {
                            state.phase = UpdatePhase::Error;
                            state.error = Some(e);
                            publish(&core, &state);
                        }
                    }
                }
            },
            Some((received, total)) = progress.recv() => {
                if state.phase == UpdatePhase::Download {
                    state.received = received;
                    state.total = total;
                    publish(&core, &state);
                }
            }
            Some(result) = done.recv() => {
                running = None;
                match result {
                    Ok(()) => state.phase = UpdatePhase::Ready,
                    Err(e) => {
                        eprintln!("[update] download failed: {e}");
                        state.phase = UpdatePhase::Error;
                        state.error = Some(e);
                        file = None;
                    }
                }
                publish(&core, &state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_feed_gives_version_commit_run_and_this_machines_build() {
        let feed = json!({
            "tag_name": "v9.1.0",
            "name": "LIVI nightly #482",
            "target_commitish": "abc1234",
            "assets": [
                { "name": "LIVI-9.1.0-arm64.AppImage", "browser_download_url": "https://x/arm" },
                { "name": "LIVI-9.1.0-x86_64.AppImage", "browser_download_url": "https://x/x64" },
                { "name": "LIVI-9.1.0-arm64.dmg", "browser_download_url": "https://x/dmg" },
                { "name": "LIVI-9.1.0-android-arm64.apk", "browser_download_url": "https://x/apk" }
            ]
        });
        let r = release(&feed);
        assert_eq!(
            (r.version.as_str(), r.commit.as_str(), r.run.as_str()),
            ("9.1.0", "abc1234", "482")
        );
        let assets = feed["assets"].as_array().unwrap();
        assert_eq!(
            pick_asset(assets, "x86_64", ".appimage").as_deref(),
            Some("https://x/x64")
        );
        assert_eq!(
            pick_asset(assets, "aarch64", ".appimage").as_deref(),
            Some("https://x/arm")
        );
        assert_eq!(
            pick_asset(assets, "aarch64", ".dmg").as_deref(),
            Some("https://x/dmg")
        );
        assert_eq!(pick_asset(assets, "x86_64", ".dmg"), None);
        assert_eq!(
            pick_asset(assets, "aarch64", ".apk").as_deref(),
            Some("https://x/apk")
        );
        assert_eq!(pick_asset(assets, "riscv64", ".appimage"), None);
        assert_eq!(
            pick_asset(
                &[json!({ "name": "LIVIx64.AppImage" })],
                "x86_64",
                ".appimage"
            ),
            None
        );
        assert_eq!(release(&json!({ "name": "V2.0" })).version, "2.0");
        assert_eq!(run_number("no number"), "");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_swap_survives_quotes_in_the_paths() {
        let script = swap_as_admin(
            Path::new("/Volumes/LIVI 2/LIVI.app"),
            Path::new("/Apps/it's.app"),
        );
        assert!(script.starts_with("do shell script \"sh -c 'set -e"));
        assert!(script.ends_with("'/Apps/it'\\\\''s.app'\" with administrator privileges"));
        assert!(script.contains(r#"src=\"$1\""#));
    }

    #[test]
    fn the_channel_picks_the_feed() {
        assert!(feed_url(false).ends_with("/releases/latest"));
        assert!(feed_url(true).ends_with("/releases/tags/nightly"));
    }
}
