// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ffi::OsString;
use std::path::PathBuf;

pub const SOCKET_ENV: &str = "LIVI_CORE_SOCKET";

pub fn runtime_dir(xdg_runtime: Option<OsString>, uid: u32) -> PathBuf {
    match xdg_runtime.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("livi"),
        None => std::env::temp_dir().join(format!("livi-{uid}")),
    }
}

pub fn core_socket(uid: u32) -> PathBuf {
    match std::env::var_os(SOCKET_ENV).filter(|v| !v.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => runtime_dir(std::env::var_os("XDG_RUNTIME_DIR"), uid).join("core.sock"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_dir_prefers_the_session_dir() {
        assert_eq!(
            runtime_dir(Some("/run/user/1000".into()), 1000),
            PathBuf::from("/run/user/1000/livi")
        );
        assert_eq!(
            runtime_dir(Some("".into()), 7),
            std::env::temp_dir().join("livi-7")
        );
        assert_eq!(runtime_dir(None, 7), std::env::temp_dir().join("livi-7"));
    }
}
