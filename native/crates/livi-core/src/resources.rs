// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};

pub struct Resources {
    pub helper: PathBuf,
    pub gst_host: PathBuf,
    pub compositor: PathBuf,
    pub gstreamer: Option<PathBuf>,
    pub templates: PathBuf,
    pub ui: Option<UiCmd>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiCmd {
    Exec { bin: PathBuf, args: Vec<String> },
    Shell(String),
}

fn platform_dir() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) => Some("macos-arm64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("linux", "x86_64") => Some("linux-x64"),
        _ => None,
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn env_str(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn repo_around(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|dir| dir.join("native/Cargo.toml").is_file())
        .map(Path::to_path_buf)
}

impl Resources {
    pub fn from_env() -> Self {
        let layout = match env_path("LIVI_RESOURCES") {
            Some(res) => Self::installed(&res),
            None => {
                let root = env_path("LIVI_ROOT")
                    .or_else(|| {
                        std::env::current_exe()
                            .ok()
                            .and_then(|exe| repo_around(&exe))
                    })
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_default();
                Self::repo(&root)
            }
        };
        let ui = if std::env::var("LIVI_NO_UI").is_ok_and(|v| v == "1") {
            None
        } else {
            env_str("LIVI_UI_CMD").map(UiCmd::Shell).or(layout.ui)
        };
        Self {
            helper: env_path("LIVI_HELPER_BIN").unwrap_or(layout.helper),
            gst_host: env_path("LIVI_GST_HOST_BIN").unwrap_or(layout.gst_host),
            compositor: env_path("LIVI_COMPOSITOR_BIN").unwrap_or(layout.compositor),
            gstreamer: layout.gstreamer,
            templates: layout.templates,
            ui,
        }
    }

    fn installed(res: &Path) -> Self {
        Self {
            helper: res.join("driver/livi-helperd"),
            gst_host: res.join("gst-host/livi-gst-host"),
            compositor: res.join("compositor/livi-compositor"),
            gstreamer: platform_dir()
                .map(|d| res.join("gstreamer").join(d))
                .filter(|p| p.is_dir()),
            templates: res.to_path_buf(),
            ui: res.parent().map(|app| UiCmd::Exec {
                bin: app.join("livi-ui"),
                args: Vec::new(),
            }),
        }
    }

    fn repo(root: &Path) -> Self {
        Self {
            helper: root.join("out/bin/livi-helperd"),
            gst_host: root.join("out/bin/livi-gst-host"),
            compositor: root.join("out/bin/livi-compositor"),
            gstreamer: platform_dir()
                .map(|d| root.join("assets/gstreamer").join(d))
                .filter(|p| p.join("lib").is_dir()),
            templates: root.join("assets/linux"),
            ui: Some(UiCmd::Exec {
                bin: root.join("out/bin/LIVI"),
                args: Vec::new(),
            }),
        }
    }
}

const LIBVA_MIN_MINOR: u32 = 21;
const LIBVA_DIRS: [&str; 4] = [
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
    "/usr/lib64",
    "/usr/lib",
];

fn libva_minor(file: &str) -> Option<u32> {
    let v: u32 = file
        .strip_prefix("libva.so.2.")?
        .split('.')
        .next()?
        .parse()
        .ok()?;
    Some(v / 100)
}

fn host_libva_ok() -> bool {
    LIBVA_DIRS.iter().map(Path::new).any(|dir| {
        ["libva-drm.so.2", "libva-x11.so.2"]
            .iter()
            .all(|l| dir.join(l).exists())
            && std::fs::canonicalize(dir.join("libva.so.2"))
                .ok()
                .and_then(|p| libva_minor(p.file_name()?.to_str()?))
                .is_some_and(|m| m >= LIBVA_MIN_MINOR)
    })
}

pub fn gst_env(root: &Path) -> Vec<(String, String)> {
    gst_env_with(root, cfg!(target_os = "linux") && !host_libva_ok())
}

fn gst_env_with(root: &Path, libva_compat: bool) -> Vec<(String, String)> {
    let utf8 = if cfg!(target_os = "macos") {
        "en_US.UTF-8"
    } else {
        "C.UTF-8"
    };
    let lib_var = if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let p = |rel: &str| root.join(rel).display().to_string();
    let compat = root.join("lib/libva-compat");
    let libs = if libva_compat && compat.is_dir() {
        format!("{}:{}", compat.display(), p("lib"))
    } else {
        p("lib")
    };
    vec![
        ("LANG".into(), utf8.into()),
        ("LC_ALL".into(), utf8.into()),
        ("GST_PLUGIN_SYSTEM_PATH".into(), String::new()),
        ("GST_PLUGIN_PATH".into(), p("lib/gstreamer-1.0")),
        (
            "GST_PLUGIN_SCANNER".into(),
            p("libexec/gstreamer-1.0/gst-plugin-scanner"),
        ),
        (lib_var.into(), libs),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_point_where_the_build_puts_things() {
        let repo = Resources::repo(Path::new("/src/LIVI"));
        assert_eq!(repo.helper, PathBuf::from("/src/LIVI/out/bin/livi-helperd"));
        assert_eq!(
            repo.compositor,
            PathBuf::from("/src/LIVI/out/bin/livi-compositor")
        );
        assert_eq!(
            repo.ui,
            Some(UiCmd::Exec {
                bin: PathBuf::from("/src/LIVI/out/bin/LIVI"),
                args: Vec::new(),
            })
        );
        let installed = Resources::installed(Path::new("/opt/LIVI/resources"));
        assert_eq!(
            installed.helper,
            PathBuf::from("/opt/LIVI/resources/driver/livi-helperd")
        );
        assert_eq!(
            installed.gst_host,
            PathBuf::from("/opt/LIVI/resources/gst-host/livi-gst-host")
        );
        assert_eq!(installed.gstreamer, None);
        assert_eq!(
            installed.ui,
            Some(UiCmd::Exec {
                bin: PathBuf::from("/opt/LIVI/livi-ui"),
                args: Vec::new()
            })
        );
    }

    #[test]
    fn a_build_in_a_checkout_finds_the_checkout() {
        let dir = crate::config_file::tests::TempDir::new();
        let release = dir.0.join("out/bin");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::create_dir_all(dir.0.join("native")).unwrap();
        std::fs::write(dir.0.join("native/Cargo.toml"), "").unwrap();
        assert_eq!(repo_around(&release.join("livi-core")), Some(dir.0.clone()));
        assert_eq!(repo_around(Path::new("/usr/bin/livi-core")), None);
    }

    #[test]
    fn gst_env_points_into_the_bundle() {
        let env = gst_env(Path::new("/b"));
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("GST_PLUGIN_PATH"), Some("/b/lib/gstreamer-1.0"));
        assert_eq!(get("GST_PLUGIN_SYSTEM_PATH"), Some(""));
    }

    #[test]
    fn libva_minor_comes_from_the_real_file_name() {
        assert_eq!(libva_minor("libva.so.2.2000.0"), Some(20));
        assert_eq!(libva_minor("libva.so.2.2400.0"), Some(24));
        assert_eq!(libva_minor("libva.so.2"), None);
        assert_eq!(libva_minor("libva-drm.so.2.2400.0"), None);
    }

    #[test]
    fn the_bundled_libva_only_goes_first_when_asked_for() {
        let dir = crate::config_file::tests::TempDir::new();
        std::fs::create_dir_all(dir.0.join("lib/libva-compat")).unwrap();
        let libs = |compat| {
            gst_env_with(&dir.0, compat)
                .into_iter()
                .find(|(n, _)| n == "LD_LIBRARY_PATH" || n == "DYLD_LIBRARY_PATH")
                .map(|(_, v)| v)
                .unwrap()
        };
        let lib = dir.0.join("lib").display().to_string();
        assert_eq!(libs(false), lib);
        assert_eq!(
            libs(true),
            format!("{}:{lib}", dir.0.join("lib/libva-compat").display())
        );
    }
}
