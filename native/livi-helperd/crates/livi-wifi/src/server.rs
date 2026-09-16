//! LIVI-Link wifid wire: line-oriented TCP `:5001`. Commands:
//!   channels | status | on | off | apply | save
//!   set <ssid|country|channel|passphrase> <value>
//!   bt on | bt off
//! Responses end in `ok\n` or `error <reason>\n`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::listing;

pub const PORT: u16 = 5001;
const RADIO_TRIES: u32 = 40;
const RADIO_POLL: Duration = Duration::from_millis(500);
const START_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(250);

const HOSTAPD: &str = "/usr/sbin/hostapd";
const IFACE: &str = "wlan0";
const BT: &str = "hci0";
const BT_DEV: u16 = 0;

/// The hostapd instance the daemon manages, with the paths it works on.
pub type OnSave = Box<dyn Fn() + Send + Sync>;

pub struct Ap {
    base: PathBuf,
    live: [PathBuf; 2],
    log: PathBuf,
    config: PathBuf,
    hostapd: Option<Child>,
    on_save: Option<OnSave>,
}

impl Ap {
    pub fn new<P: Into<PathBuf>>(base: P, live: [P; 2], log: P) -> Self {
        let base = base.into();
        let live = live.map(Into::into);
        Self {
            config: base.clone(),
            base,
            live,
            log: log.into(),
            hostapd: None,
            on_save: None,
        }
    }

    pub fn with_on_save(mut self, on_save: OnSave) -> Self {
        self.on_save = Some(on_save);
        self
    }

    pub fn config_path(&self) -> &PathBuf {
        &self.config
    }
}

#[derive(Default)]
struct Wanted {
    ssid: Option<String>,
    country: Option<String>,
    channel: Option<u32>,
    passphrase: Option<String>,
}

enum Cmd<'a> {
    Channels,
    Status,
    Set(&'a str, &'a str),
    Apply,
    Save,
    On,
    Off,
    Bt(bool),
    Empty,
    Unknown(&'a str),
}

pub fn serve<S: std::io::Read + Write>(io: &mut S, ap: &mut Ap) {
    let mut reader = BufReader::new(io);
    let mut wanted = Wanted::default();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let answer = match command(line.trim_end_matches(['\r', '\n'])) {
            Cmd::Channels => match listing() {
                Ok(text) => format!("{text}ok\n"),
                Err(e) => format!("error {e}\n"),
            },
            Cmd::Status => status(ap),
            Cmd::Set(key, value) => match remember(&mut wanted, key, value) {
                Ok(()) => "ok\n".into(),
                Err(e) => format!("error {e}\n"),
            },
            Cmd::Apply => {
                let answer = match apply(ap, &wanted) {
                    Ok(()) => "ok\n".into(),
                    Err(e) => format!("error {e}\n"),
                };
                wanted = Wanted::default();
                answer
            }
            Cmd::On => match on(ap) {
                Ok(()) => "ok\n".into(),
                Err(e) => format!("error {e}\n"),
            },
            Cmd::Off => {
                off(ap);
                "ok\n".into()
            }
            Cmd::Save => match save(ap) {
                Ok(()) => "ok\n".into(),
                Err(e) => format!("error {e}\n"),
            },
            Cmd::Bt(up) => match bluetooth(up) {
                Ok(()) => "ok\n".into(),
                Err(e) => format!("error {e}\n"),
            },
            Cmd::Empty => continue,
            Cmd::Unknown(what) => format!("error unknown command {what}\n"),
        };
        if reader.get_mut().write_all(answer.as_bytes()).is_err() {
            return;
        }
    }
}

fn command(line: &str) -> Cmd<'_> {
    let line = line.trim();
    let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
    match head {
        "" => Cmd::Empty,
        "channels" => Cmd::Channels,
        "status" => Cmd::Status,
        "apply" => Cmd::Apply,
        "save" => Cmd::Save,
        "on" => Cmd::On,
        "off" => Cmd::Off,
        "bt" => match rest.trim() {
            "on" => Cmd::Bt(true),
            "off" => Cmd::Bt(false),
            _ => Cmd::Unknown(line),
        },
        "set" => match rest.split_once(' ') {
            Some((key, value)) => Cmd::Set(key, value),
            None => Cmd::Unknown(line),
        },
        _ => Cmd::Unknown(head),
    }
}

fn remember(wanted: &mut Wanted, key: &str, value: &str) -> Result<(), String> {
    if value.contains(['\n', '\r']) {
        return Err("a value holds a line break".into());
    }
    match key {
        "ssid" => {
            if value.is_empty() || value.len() > 32 {
                return Err("ssid must be 1 to 32 bytes".into());
            }
            wanted.ssid = Some(value.to_string());
        }
        "country" => {
            if value.len() != 2 || !value.bytes().all(|b| b.is_ascii_alphabetic()) {
                return Err("country must be two letters".into());
            }
            wanted.country = Some(value.to_ascii_uppercase());
        }
        "channel" => {
            let channel = value.parse::<u32>().map_err(|_| "channel must be a number")?;
            if !(1..=196).contains(&channel) {
                return Err("channel is out of range".into());
            }
            wanted.channel = Some(channel);
        }
        "passphrase" => {
            if !(8..=63).contains(&value.len()) {
                return Err("passphrase must be 8 to 63 bytes".into());
            }
            wanted.passphrase = Some(value.to_string());
        }
        other => return Err(format!("unknown setting {other}")),
    }
    Ok(())
}

fn config(base: &str, wanted: &Wanted) -> String {
    // A base with 802.11ac pins an 80 MHz block (vht_oper_centr_freq_seg0_idx) to its own
    // channel; any other channel makes hostapd refuse to start. Regenerated as VHT40 below,
    // which needs no centre index, and dropped entirely on 2.4 GHz.
    let vht = base
        .lines()
        .any(|line| setting(line) == Some("ieee80211ac") && line.trim().ends_with("=1"));
    let mut out = String::new();
    for line in base.lines() {
        let replaced = match setting(line) {
            Some("ssid") => wanted.ssid.is_some(),
            Some("country_code") => wanted.country.is_some(),
            Some(
                "channel" | "hw_mode" | "ht_capab" | "vendor_elements" | "assocresp_elements"
                | "ieee80211ac" | "vht_capab" | "vht_oper_chwidth" | "vht_oper_centr_freq_seg0_idx",
            ) => wanted.channel.is_some(),
            Some("wpa_passphrase") => wanted.passphrase.is_some(),
            _ => false,
        };
        if !replaced {
            out.push_str(line);
            out.push('\n');
        }
    }
    if let Some(country) = &wanted.country {
        out.push_str(&format!("country_code={country}\n"));
    }
    if let Some(channel) = wanted.channel {
        let ie = apple_ie(channel);
        out.push_str(&format!(
            "hw_mode={}\nchannel={channel}\nht_capab=[SHORT-GI-20][SHORT-GI-40]{}\n\
             vendor_elements={ie}\nassocresp_elements={ie}\n",
            band(channel),
            ht40(channel)
        ));
        if vht && band(channel) == "a" {
            out.push_str("ieee80211ac=1\nvht_oper_chwidth=0\n");
        }
    }
    if let Some(ssid) = &wanted.ssid {
        out.push_str(&format!("ssid={ssid}\n"));
    }
    if let Some(passphrase) = &wanted.passphrase {
        out.push_str(&format!("wpa_passphrase={passphrase}\n"));
    }
    out
}

fn setting(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    line.split_once('=').map(|(key, _)| key.trim())
}

fn await_radio() -> Result<(), String> {
    let path = format!("/sys/class/net/{IFACE}");
    for _ in 0..RADIO_TRIES {
        if std::path::Path::new(&path).exists() {
            return Ok(());
        }
        std::thread::sleep(RADIO_POLL);
    }
    Err(format!("{IFACE} never appeared"))
}

fn band(channel: u32) -> &'static str {
    if channel <= 14 { "g" } else { "a" }
}

fn apple_ie(channel: u32) -> String {
    let band_bit: u8 = if channel >= 36 { 0x01 } else { 0x02 };
    format!("dd0800a04000000200{:02x}", 0x20 | band_bit)
}

fn ht40(channel: u32) -> &'static str {
    let up = if channel <= 14 {
        channel <= 7
    } else {
        (channel / 4) % 2 == 1
    };
    if up { "[HT40+]" } else { "[HT40-]" }
}

fn apply(ap: &mut Ap, wanted: &Wanted) -> Result<(), String> {
    await_radio()?;
    let base = std::fs::read_to_string(&ap.base)
        .map_err(|e| format!("{}: {e}", ap.base.display()))?;
    if let Some(parent) = ap.live[0].parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = config(&base, wanted);
    if running() && std::fs::read_to_string(&ap.config).is_ok_and(|current| current == text) {
        return Ok(());
    }
    let next = if ap.config == ap.live[0] { ap.live[1].clone() } else { ap.live[0].clone() };
    std::fs::write(&next, text).map_err(|e| format!("{}: {e}", next.display()))?;

    let previous = ap.config.clone();
    stop(ap);
    if let Err(refused) = start(ap, &next) {
        // Gone, so the newest live file is always the one the radio runs on.
        let _ = std::fs::remove_file(&next);
        stop(ap);
        if start(ap, &previous).is_err() {
            stop(ap);
            let base = ap.base.clone();
            let _ = start(ap, &base);
            ap.config = ap.base.clone();
        }
        return Err(refused);
    }
    ap.config = next;
    Ok(())
}

fn save(ap: &Ap) -> Result<(), String> {
    if ap.config == ap.base {
        return Ok(());
    }
    let live = std::fs::read_to_string(&ap.config)
        .map_err(|e| format!("{}: {e}", ap.config.display()))?;
    let base = std::fs::read_to_string(&ap.base)
        .map_err(|e| format!("{}: {e}", ap.base.display()))?;
    let next = config(&base, &settings_of(&live));
    if next == base {
        return Ok(());
    }
    let temp = ap.base.with_extension("new");
    std::fs::write(&temp, next).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, &ap.base).map_err(|e| format!("{}: {e}", ap.base.display()))?;
    let _ = Command::new("sync").status();
    if let Some(cb) = ap.on_save.as_ref() {
        cb();
    }
    Ok(())
}

fn settings_of(text: &str) -> Wanted {
    let value = |key: &str| {
        text.lines()
            .rfind(|line| setting(line) == Some(key))
            .and_then(|line| line.split_once('='))
            .map(|(_, value)| value.trim().to_string())
    };
    Wanted {
        ssid: value("ssid"),
        country: value("country_code"),
        channel: value("channel").and_then(|c| c.parse().ok()),
        passphrase: value("wpa_passphrase"),
    }
}

fn on(ap: &mut Ap) -> Result<(), String> {
    if running() {
        return Ok(());
    }
    let _ = Command::new("ifconfig").args([IFACE, "up"]).status();
    let config = ap.config.clone();
    start(ap, &config)
}

fn off(ap: &mut Ap) {
    stop(ap);
    let _ = Command::new("ifconfig").args([IFACE, "down"]).status();
}

fn bluetooth(up: bool) -> Result<(), String> {
    let (what, result) = if up {
        ("up", livi_btd::hci::up(BT_DEV))
    } else {
        ("down", livi_btd::hci::down(BT_DEV))
    };
    result.map_err(|e| format!("{BT} would not go {what}: {e}"))
}

fn bt_up() -> bool {
    livi_btd::hci::is_up(BT_DEV)
}

/// The hostapd config the radio runs on: the newest live file, or the base when none exists.
pub fn ap_config_from(base: &std::path::Path, live: &[&std::path::Path]) -> Option<String> {
    let newest = live
        .iter()
        .copied()
        .filter_map(|file| {
            let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok()?;
            Some((modified, file))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, file)| file)
        .unwrap_or(base);
    std::fs::read_to_string(newest).ok()
}

pub fn ap_name_from(base: &std::path::Path, live: &[&std::path::Path]) -> Option<String> {
    let text = ap_config_from(base, live)?;
    text.lines()
        .filter(|line| setting(line) == Some("ssid"))
        .filter_map(|line| line.split_once('=').map(|(_, v)| v.trim().to_string()))
        .find(|value| !value.is_empty())
}

fn status(ap: &Ap) -> String {
    let mut out = String::new();
    out.push_str(if running() { "state on\n" } else { "state off\n" });
    out.push_str(if bt_up() { "bt on\n" } else { "bt off\n" });
    if let Ok(mac) = std::fs::read_to_string(format!("/sys/class/net/{IFACE}/address")) {
        out.push_str(&format!("mac {}\n", mac.trim()));
    }
    if let Ok(mac) = std::fs::read_to_string(format!("/sys/class/bluetooth/{BT}/address")) {
        out.push_str(&format!("btmac {}\n", mac.trim()));
    }
    out.push_str(if ap.config == ap.base { "config fallback\n" } else { "config host\n" });
    if let Ok(text) = std::fs::read_to_string(&ap.config) {
        for line in text.lines() {
            if let Some(key @ ("ssid" | "country_code" | "channel" | "hw_mode")) = setting(line) {
                let value = line.split_once('=').map(|(_, v)| v).unwrap_or("");
                out.push_str(&format!("{key} {value}\n"));
            }
        }
    }
    // Link telemetry from the car's point of view: down = phone→car, up = car→phone. The PHY
    // bitrate is the connected station's negotiated rate; the byte counters let the host derive
    // throughput by sampling over time. All absent until a phone is on the air.
    if let Some((down, up)) = crate::station_rates(IFACE) {
        out.push_str(&format!("downrate {down}\nuprate {up}\n"));
    }
    let counter = |dir: &str| {
        std::fs::read_to_string(format!("/sys/class/net/{IFACE}/statistics/{dir}_bytes"))
            .ok()
            .map(|s| s.trim().to_string())
    };
    // wlan0 RX is what the AP received from the phone (down); TX is what it sent (up).
    if let Some(bytes) = counter("rx") {
        out.push_str(&format!("downbytes {bytes}\n"));
    }
    if let Some(bytes) = counter("tx") {
        out.push_str(&format!("upbytes {bytes}\n"));
    }
    out.push_str("ok\n");
    out
}

fn start(ap: &mut Ap, config: &std::path::Path) -> Result<(), String> {
    let _ = std::fs::remove_file(&ap.log);
    let log = std::fs::File::create(&ap.log)
        .map_err(|e| format!("{}: {e}", ap.log.display()))?;
    let errors = log.try_clone().map_err(|e| e.to_string())?;
    let mut child = Command::new(HOSTAPD)
        .arg(config)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .spawn()
        .map_err(|e| format!("hostapd: {e}"))?;

    let deadline = std::time::Instant::now() + START_TIMEOUT;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(POLL);
        let text = std::fs::read_to_string(&ap.log).unwrap_or_default();
        if text.contains("AP-ENABLED") {
            ap.hostapd = Some(child);
            return Ok(());
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            return Err(complaint(&text));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("hostapd did not bring the radio up".into())
}

fn stop(ap: &mut Ap) {
    let _ = Command::new("killall").arg("hostapd").status();
    if let Some(mut child) = ap.hostapd.take() {
        let _ = child.wait();
    }
    for _ in 0..20 {
        if !running() {
            return;
        }
        std::thread::sleep(POLL);
    }
}

fn complaint(log: &str) -> String {
    const MARKERS: [&str; 5] = ["not allowed", "Could not", "Unable", "Invalid", "ailed"];
    log.lines()
        .map(str::trim)
        .find(|line| MARKERS.iter().any(|m| line.contains(m)))
        .or_else(|| log.lines().map(str::trim).rev().find(|line| !line.is_empty()))
        .unwrap_or("hostapd failed")
        .to_string()
}

fn running() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else { return false; };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm"))
            && comm.trim() == "hostapd"
        {
            return true;
        }
    }
    false
}
