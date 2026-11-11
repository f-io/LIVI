// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::process::Command;

use crate::http::{S_200, S_400, S_500, T_JSON, err_json, json_num, json_str, ok_json, url_decode};

pub(crate) const LED_CFG: &str = "/tmp/livi/led.toml";

pub(crate) const LED_PID: &str = "/tmp/livi/livi-ledd.pid";

pub(crate) fn led_json() -> String {
    let mut r = 0x4du8;
    let mut g = 0xd0u8;
    let mut b = 0xe1u8;
    let mut brightness = 20u8;
    if let Ok(s) = fs::read_to_string(LED_CFG) {
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "status_color" => {
                    if let Some((rr, gg, bb)) = parse_rgb(v) {
                        r = rr;
                        g = gg;
                        b = bb;
                    }
                }
                "brightness" => {
                    if let Ok(n) = v.parse::<u8>() {
                        brightness = n.min(100);
                    }
                }
                _ => {}
            }
        }
    }
    format!(
        r##"{{"status_color":"#{:02x}{:02x}{:02x}","brightness":{}}}"##,
        r, g, b, brightness
    )
}

pub(crate) fn set_led(body: Option<&[u8]>) -> (&'static str, &'static str, Vec<u8>) {
    let Some(data) = body else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    let Ok(s) = std::str::from_utf8(data) else {
        return (S_400, T_JSON, err_json("body not UTF-8"));
    };

    let mut status: Option<(u8, u8, u8)> = None;
    let mut brightness: Option<u8> = None;

    let trimmed = s.trim();
    if trimmed.starts_with('{') {
        if let Some(v) = json_str(trimmed, "status_color") {
            status = parse_rgb(&v);
        }
        if let Some(v) = json_num(trimmed, "brightness") {
            brightness = v.parse::<u8>().ok().map(|n| n.min(100));
        }
    } else {
        for kv in trimmed.split('&') {
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            let v = url_decode(v);
            match k {
                "status_color" => status = parse_rgb(&v),
                "brightness" => brightness = v.parse::<u8>().ok().map(|n| n.min(100)),
                _ => {}
            }
        }
    }

    let existing = led_json();
    let cur_status = json_str(&existing, "status_color")
        .and_then(|v| parse_rgb(&v))
        .unwrap_or((0x4d, 0xd0, 0xe1));
    let cur_bri = json_num(&existing, "brightness")
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(20);

    let (r, g, b) = status.unwrap_or(cur_status);
    let bri = brightness.unwrap_or(cur_bri);

    let cur_wb = fs::read_to_string(LED_CFG)
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                let l = l.trim();
                if l.starts_with('#') {
                    return None;
                }
                let (k, v) = l.split_once('=')?;
                (k.trim() == "white_balance").then(|| v.trim().trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "#ffbe82".to_string());

    let contents = format!(
        "# LIVI-Link LED config (managed by livi-httpd). brightness is 0-100 %.\n\
         status_color = \"#{:02x}{:02x}{:02x}\"\n\
         brightness = {}\n\
         white_balance = \"{}\"\n",
        r, g, b, bri, cur_wb
    );

    let _ = fs::create_dir_all("/tmp/livi");
    if let Err(e) = fs::write(LED_CFG, contents) {
        return (S_500, T_JSON, err_json(&format!("write {LED_CFG}: {e}")));
    }
    kick_ledd();
    persist_config();
    (S_200, T_JSON, ok_json("led config updated"))
}

pub(crate) fn kick_ledd() {
    let Ok(s) = fs::read_to_string(LED_PID) else {
        return;
    };
    let Ok(pid) = s.trim().parse::<i32>() else {
        return;
    };
    if pid <= 0 {
        return;
    }
    // SAFETY: kill takes plain ints and touches no memory of ours.
    unsafe {
        libc::kill(pid, libc::SIGHUP);
    }
}

pub(crate) fn persist_config() {
    let _ = Command::new("/usr/bin/livid")
        .arg("config")
        .arg("save")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub(crate) fn parse_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some((r, g, b));
    }
    let parts: Vec<_> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}
