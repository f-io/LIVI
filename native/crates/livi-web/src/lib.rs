// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#![cfg(target_os = "linux")]

mod flash;
mod http;
mod led;
mod status;
mod update;

use std::sync::OnceLock;

use livi_link_ap::radio::Radio;

use flash::{flash_dispatch, flash_status_json, reboot_soon};
use http::{INDEX_HTML, S_200, S_404, T_HTML, T_JSON, T_TEXT, serve_forever};
use led::{led_json, set_led};
use status::{bt_json, caps_json, status_json, switch_radio, wifi_json};
use update::{set_update, update_json};

#[derive(Default)]
pub struct Flash {
    pub mtd: Vec<MtdSlot>,
    pub check: Option<String>,
}

pub struct MtdSlot {
    pub typ: u8,
    pub node: String,
    pub magic: Vec<u8>,
    pub size: u64,
    pub stage: Option<Stage>,
    pub before_write: Option<fn() -> Result<(), String>>,
}

pub type Stage = fn(&[u8], &[u8]) -> Result<Vec<u8>, String>;

pub struct WebCaps {
    pub model: String,
    pub target: String,
    pub port: u16,
    pub wifi_iface: String,
    pub host_iface: String,
    pub mfi: String,
    pub bt: String,
    pub led: bool,
    pub flash: Flash,
    pub update_conf: String,
}

const VERSION: &str = match option_env!("LIVI_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

const BUILD: &str = match option_env!("LIVI_BUILD") {
    Some(b) => b,
    None => "dev",
};

static CAPS: OnceLock<WebCaps> = OnceLock::new();

fn caps() -> &'static WebCaps {
    CAPS.get()
        .expect("livi_web::run must be called before any handler")
}

pub fn run(caps: WebCaps) -> i32 {
    let _ = CAPS.set(caps);
    match serve_forever() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[livi-web] {e}");
            1
        }
    }
}

fn route(
    rmethod: &str,
    rpath: &str,
    body: Option<&[u8]>,
    clen: u64,
) -> (&'static str, &'static str, Vec<u8>) {
    let c = caps();
    let path = rpath.split_once('?').map_or(rpath, |(p, _)| p);
    let method = rmethod;

    if c.led {
        match (method, path) {
            ("GET", "/api/led") => return (S_200, T_JSON, led_json().into_bytes()),
            ("POST", "/api/led") => return set_led(body),
            _ => {}
        }
    }
    if !c.flash.mtd.is_empty() && (method, path) == ("POST", "/api/flash") {
        return flash_dispatch(c, body, clen);
    }

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => (S_200, T_HTML, INDEX_HTML.as_bytes().to_vec()),
        ("GET", "/api/status") => (S_200, T_JSON, status_json().into_bytes()),
        ("GET", "/api/wifi") => (S_200, T_JSON, wifi_json().into_bytes()),
        ("POST", "/api/wifi") => switch_radio(body, Radio::Wifi),
        ("GET", "/api/bt") => (S_200, T_JSON, bt_json().into_bytes()),
        ("POST", "/api/bt") => switch_radio(body, Radio::Bt),
        ("GET", "/api/caps") => (S_200, T_JSON, caps_json().into_bytes()),
        ("GET", "/api/flash/status") => (S_200, T_JSON, flash_status_json().into_bytes()),
        ("GET", "/api/update") => (S_200, T_JSON, update_json().into_bytes()),
        ("POST", "/api/update") => set_update(body),
        ("POST", "/api/reboot") => reboot_soon(),
        _ => (S_404, T_TEXT, b"not found\n".to_vec()),
    }
}
