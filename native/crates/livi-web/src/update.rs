// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;

use crate::caps;
use crate::http::{S_200, S_400, S_500, T_JSON, err_json, json_num, ok_json};
use crate::led::persist_config;

pub(crate) fn update_nightly() -> bool {
    fs::read_to_string(&caps().update_conf)
        .is_ok_and(|s| s.lines().any(|l| l.trim() == "nightly=1"))
}

pub(crate) fn update_json() -> String {
    format!(r#"{{"nightly":{}}}"#, update_nightly())
}

pub(crate) fn set_update(body: Option<&[u8]>) -> (&'static str, &'static str, Vec<u8>) {
    let Some(s) = body.and_then(|b| std::str::from_utf8(b).ok()) else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    let nightly = match json_num(s.trim(), "nightly").as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => return (S_400, T_JSON, err_json("nightly must be 0 or 1")),
    };
    let path = &caps().update_conf;
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Err(e) = fs::write(path, format!("nightly={}\n", u8::from(nightly))) {
        return (S_500, T_JSON, err_json(&format!("{path}: {e}")));
    }
    persist_config();
    (S_200, T_JSON, ok_json("update channel saved"))
}
