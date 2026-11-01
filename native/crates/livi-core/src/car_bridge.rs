// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::message::MediaControl;
use livi_core_proto::state::{Navigation, NowPlaying};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::nav_text::{self, Language};
use crate::server::Core;

const PRODUCT: &str = "LIVI Bridge";
const RETRY: Duration = Duration::from_secs(3);
const MEDIA_BLANK: Duration = Duration::from_secs(4);
const LINE_MAX: usize = 512;
const MEDIA_TEXTS: [&str; 4] = ["media title", "media artist", "media album", "media app"];

#[derive(Default)]
struct Rows {
    rows: Vec<(String, String)>,
    sent: HashMap<String, String>,
}

impl Rows {
    fn set(&mut self, field: &str, value: &str) {
        let value = value.replace(['\n', '\r'], " ");
        match self.rows.iter_mut().find(|(f, _)| f == field) {
            Some(row) => row.1 = value,
            None => self.rows.push((field.to_string(), value)),
        }
    }

    fn unsent(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for (field, value) in &self.rows {
            if self.sent.get(field) != Some(value) {
                self.sent.insert(field.clone(), value.clone());
                out.push(format!("{field} {value}\n"));
            }
        }
        out
    }
}

fn text(rows: &mut Rows, field: &str, value: &Option<String>) {
    if let Some(v) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        rows.set(field, v);
    }
}

fn media(rows: &mut Rows, np: &NowPlaying) {
    text(rows, "media title", &np.title);
    text(rows, "media artist", &np.artist);
    text(rows, "media album", &np.album);
    text(rows, "media app", &np.app);
    if let Some(ms) = np.duration_ms {
        rows.set("media duration", &ms.to_string());
    }
    if let Some(ms) = np.elapsed_ms {
        rows.set("media position", &ms.to_string());
    }
    if let Some(playing) = np.playing {
        rows.set("media playing", if playing { "true" } else { "false" });
    }
}

fn nav(rows: &mut Rows, nav: &Navigation, language: Language) {
    if nav.active != Some(true) {
        rows.set("nav active", "false");
        return;
    }
    rows.set("nav active", "true");
    if let Some(code) = nav.maneuver_type {
        rows.set("nav maneuver", &code.to_string());
    }
    let maneuver = nav
        .maneuver_text
        .as_deref()
        .unwrap_or(nav_text::unknown_maneuver(language));
    rows.set("nav maneuver-text", maneuver);
    text(rows, "nav distance", &nav.maneuver_distance_text);
    text(rows, "nav road", &nav.road_name);
    text(rows, "nav destination", &nav.destination_name);
    text(
        rows,
        "nav eta",
        &nav.eta.and_then(clock).or_else(|| nav.eta_text.clone()),
    );
    if let Some(m) = nav.distance_to_destination {
        rows.set("nav destination-distance", &m.to_string());
    }
    if let Some(s) = nav.time_to_destination {
        rows.set("nav time-left", &s.to_string());
    }
}

fn clock(epoch: f64) -> Option<String> {
    let t = epoch as libc::time_t;
    // SAFETY: an all-zero tm is valid, localtime_r only writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return None;
    }
    Some(format!("{:02}:{:02}", tm.tm_hour, tm.tm_min))
}

#[derive(Debug, PartialEq)]
enum FromBridge {
    Button(MediaControl),
    Car(Value),
    Unused,
}

fn parse(line: &str) -> FromBridge {
    match line {
        "button next" => FromBridge::Button(MediaControl::Next),
        "button previous" => FromBridge::Button(MediaControl::Prev),
        "button scan" => FromBridge::Button(MediaControl::PlayPause),
        _ => line
            .strip_prefix("car ")
            .and_then(car)
            .map_or(FromBridge::Unused, FromBridge::Car),
    }
}

fn car(rest: &str) -> Option<Value> {
    let (key, raw) = rest.split_once(' ')?;
    let raw = raw.trim();
    let name = |part: &str| {
        let mut chars = part.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let parts: Vec<&str> = key.split('.').collect();
    if raw.is_empty() || parts.len() > 2 || !parts.iter().all(|p| name(p)) {
        return None;
    }
    let value = match raw {
        "true" => json!(true),
        "false" => json!(false),
        _ => match raw.parse::<f64>() {
            Ok(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 9e15 => json!(n as i64),
            Ok(n) if n.is_finite() => json!(n),
            _ => json!(raw),
        },
    };
    Some(match parts.as_slice() {
        [group, field] => json!({ *group: { *field: value } }),
        _ => json!({ key: value }),
    })
}

struct Port {
    file: File,
    lines: mpsc::UnboundedReceiver<String>,
}

impl Port {
    fn open(path: &str) -> io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(path)?;
        if let Err(e) = raw_mode(&file) {
            eprintln!("[bridge] {path} stays cooked: {e}");
        }
        let reader = file.try_clone()?;
        let (tx, lines) = mpsc::unbounded_channel();
        std::thread::spawn(move || read_lines(reader, tx));
        Ok(Self { file, lines })
    }
}

fn raw_mode(file: &File) -> io::Result<()> {
    let fd = file.as_raw_fd();
    // SAFETY: an all-zero termios is valid, tcgetattr fills it.
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: fd is open for the call and t is a valid termios.
    unsafe {
        if libc::tcgetattr(fd, &mut t) != 0 {
            return Err(io::Error::last_os_error());
        }
        libc::cfmakeraw(&mut t);
        libc::cfsetspeed(&mut t, libc::B115200);
        if libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn read_lines(mut reader: File, tx: mpsc::UnboundedSender<String>) {
    let mut buf = [0u8; 256];
    let mut pending = Vec::new();
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            return;
        }
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line).trim().to_string();
            if !line.is_empty() && tx.send(line).is_err() {
                return;
            }
        }
        if pending.len() > LINE_MAX {
            pending.drain(..pending.len() - LINE_MAX);
        }
    }
}

#[cfg(target_os = "linux")]
async fn find_port() -> Option<String> {
    let read = |p: &std::path::Path| {
        std::fs::read_to_string(p)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    for entry in std::fs::read_dir("/sys/class/tty").ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("ttyACM") {
            continue;
        }
        let Ok(iface) = std::fs::canonicalize(format!("/sys/class/tty/{name}/device")) else {
            continue;
        };
        let Some(device) = iface.parent() else {
            continue;
        };
        if read(&device.join("product")).eq_ignore_ascii_case(PRODUCT) {
            return Some(format!("/dev/{name}"));
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
async fn find_port() -> Option<String> {
    let out = tokio::process::Command::new("ioreg")
        .args(["-r", "-l", "-c", "IOUSBHostDevice"])
        .output()
        .await
        .ok()?;
    callout_device(&String::from_utf8_lossy(&out.stdout))
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn callout_device(ioreg: &str) -> Option<String> {
    let mut ours = false;
    for line in ioreg.lines() {
        if let Some(head) = line.strip_prefix("+-o ") {
            let name = head.split('@').next().unwrap_or("").trim();
            ours = name.eq_ignore_ascii_case(PRODUCT);
        } else if ours && let Some((_, path)) = line.split_once("\"IOCalloutDevice\" = \"") {
            return path.strip_suffix('"').map(str::to_string);
        }
    }
    None
}

pub async fn run(core: Arc<Core>, car_data: mpsc::UnboundedSender<Value>) {
    let mut state = core.hub.watch();
    let mut rows = Rows::default();
    let mut port: Option<Port> = None;
    let mut retry_at = Instant::now();
    let mut blank_at: Option<Instant> = None;
    let mut seen: Option<(NowPlaying, Navigation, String, f64)> = None;
    loop {
        let now = {
            let s = state.borrow_and_update();
            (
                s.now_playing.clone(),
                s.navigation.clone(),
                s.config.language.clone(),
                s.config.display_brightness,
            )
        };
        if seen.as_ref() != Some(&now) {
            let (np, navigation, language, brightness) = &now;
            let language = Language::of(language);
            rows.set("name", "LIVI");
            if brightness.is_finite() {
                rows.set(
                    "brightness",
                    &(brightness * 100.0).clamp(0.0, 100.0).round().to_string(),
                );
            }
            rows.set("nav idle-text", nav_text::no_route(language));
            if *np == NowPlaying::default() {
                let had = seen
                    .as_ref()
                    .is_some_and(|(was, ..)| *was != NowPlaying::default());
                if had {
                    blank_at = Some(Instant::now() + MEDIA_BLANK);
                }
            } else {
                blank_at = None;
                media(&mut rows, np);
            }
            nav(&mut rows, navigation, language);
            seen = Some(now);
            flush(&mut port, &mut rows);
        }
        let next_retry = retry_at;
        let blank = blank_at;
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            line = async {
                match port.as_mut() {
                    Some(p) => p.lines.recv().await,
                    None => std::future::pending().await,
                }
            } => match line {
                Some(line) => match parse(&line) {
                    FromBridge::Button(control) => core.media(control),
                    FromBridge::Car(patch) => {
                        let _ = car_data.send(patch);
                    }
                    FromBridge::Unused => {}
                },
                None => {
                    println!("[bridge] gone");
                    port = None;
                    retry_at = Instant::now() + RETRY;
                }
            },
            () = tokio::time::sleep_until(next_retry), if port.is_none() => {
                retry_at = Instant::now() + RETRY;
                let Some(path) = find_port().await else { continue };
                match Port::open(&path) {
                    Ok(p) => {
                        println!("[bridge] connected on {path}");
                        port = Some(p);
                        rows.sent.clear();
                        flush(&mut port, &mut rows);
                    }
                    Err(e) => eprintln!("[bridge] {path}: {e}"),
                }
            }
            () = async {
                match blank {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                blank_at = None;
                for field in MEDIA_TEXTS {
                    rows.set(field, "");
                }
                flush(&mut port, &mut rows);
            }
        }
    }
}

fn flush(port: &mut Option<Port>, rows: &mut Rows) {
    let Some(p) = port.as_mut() else { return };
    for line in rows.unsent() {
        if let Err(e) = p.file.write_all(line.as_bytes()) {
            eprintln!("[bridge] write failed: {e}");
            *port = None;
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_buttons_and_car_data_are_understood() {
        assert_eq!(parse("button next"), FromBridge::Button(MediaControl::Next));
        assert_eq!(
            parse("button previous"),
            FromBridge::Button(MediaControl::Prev)
        );
        assert_eq!(
            parse("button scan"),
            FromBridge::Button(MediaControl::PlayPause)
        );
        assert_eq!(
            parse("car speed 42"),
            FromBridge::Car(json!({ "speed": 42 }))
        );
        assert_eq!(
            parse("car gps.lat 52.5"),
            FromBridge::Car(json!({ "gps": { "lat": 52.5 } }))
        );
        assert_eq!(
            parse("car reverse true"),
            FromBridge::Car(json!({ "reverse": true }))
        );
        assert_eq!(parse("car gear D"), FromBridge::Car(json!({ "gear": "D" })));
        for unused in [
            "power on",
            "remote up",
            "car 1x 2",
            "car a.b.c 1",
            "car speed",
            "button x",
        ] {
            assert_eq!(parse(unused), FromBridge::Unused, "{unused}");
        }
    }

    #[test]
    fn the_bridge_is_found_by_its_name_in_the_usb_registry() {
        let ioreg = r#"+-o Arduino Uno@01100000  <class IOUSBHostDevice>
  | {
  |   "USB Product Name" = "Arduino Uno"
  | }
  | +-o IOSerialBSDClient  <class IOSerialBSDClient>
  |     "IOCalloutDevice" = "/dev/cu.usbmodem1101"
+-o LIVI bridge@01200000  <class IOUSBHostDevice>
  | +-o IOSerialBSDClient  <class IOSerialBSDClient>
  |     "IOCalloutDevice" = "/dev/cu.usbmodem1201"
"#;
        assert_eq!(
            callout_device(ioreg).as_deref(),
            Some("/dev/cu.usbmodem1201")
        );
        assert_eq!(
            callout_device("+-o Other@1\n  \"IOCalloutDevice\" = \"/dev/x\"\n"),
            None
        );
    }

    #[test]
    fn a_field_goes_out_once_and_all_again_after_a_reconnect() {
        let mut rows = Rows::default();
        rows.set("name", "LIVI");
        rows.set("media title", "Song\nTwo");
        assert_eq!(rows.unsent(), ["name LIVI\n", "media title Song Two\n"]);
        rows.set("name", "LIVI");
        assert!(rows.unsent().is_empty());
        rows.set("name", "Car");
        assert_eq!(rows.unsent(), ["name Car\n"]);
        rows.sent.clear();
        assert_eq!(rows.unsent().len(), 2);
    }

    #[test]
    fn what_plays_and_the_guidance_become_fields() {
        let mut rows = Rows::default();
        let np = NowPlaying {
            title: Some(" Song ".into()),
            artist: Some(String::new()),
            duration_ms: Some(180_000.0),
            elapsed_ms: Some(1500.5),
            playing: Some(true),
            ..Default::default()
        };
        media(&mut rows, &np);
        let guidance = Navigation {
            active: Some(true),
            maneuver_type: Some(1),
            maneuver_distance_text: Some("200 m".into()),
            road_name: Some("Main St".into()),
            eta_text: Some("12:30".into()),
            distance_to_destination: Some(5000.0),
            time_to_destination: Some(600.0),
            ..Default::default()
        };
        nav(&mut rows, &guidance, Language::of("de"));
        assert_eq!(
            rows.unsent(),
            [
                "media title Song\n",
                "media duration 180000\n",
                "media position 1500.5\n",
                "media playing true\n",
                "nav active true\n",
                "nav maneuver 1\n",
                "nav maneuver-text Unbekannt\n",
                "nav distance 200 m\n",
                "nav road Main St\n",
                "nav eta 12:30\n",
                "nav destination-distance 5000\n",
                "nav time-left 600\n",
            ]
        );
        nav(&mut rows, &Navigation::default(), Language::of("de"));
        assert_eq!(rows.unsent(), ["nav active false\n"]);
        assert!(clock(0.0).is_some_and(|c| c.len() == 5));
    }
}
