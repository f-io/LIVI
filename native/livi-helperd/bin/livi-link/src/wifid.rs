//! The access point, driven from the host over TCP. Settings are collected per connection and
//! take effect on `apply`, which puts the previous config back when hostapd refuses the new one.
//! `save` writes them into the config the dongle boots with.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::Duration;

pub const PORT: u16 = 5001;

/// The config the dongle boots with, and what `save` writes into.
const BASE: &str = "/etc/hostapd.conf";
const RADIO_TRIES: u32 = 40;
const RADIO_POLL: std::time::Duration = std::time::Duration::from_millis(500);
/// What the host asked for, in tmpfs. Two of them, so a refused config leaves the running one.
const LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];
const LOG: &str = "/tmp/livi/hostapd.log";
const HOSTAPD: &str = "/usr/sbin/hostapd";
const IFACE: &str = "wlan0";
/// The dongle's only Bluetooth controller.
const BT: &str = "hci0";

const START_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(250);

pub fn run() -> ExitCode {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[wifid] bind :{PORT}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("[wifid] listening on :{PORT}");
    let mut ap = Ap {
        config: BASE.to_string(),
        hostapd: None,
    };
    // One client at a time.
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        serve(&mut stream, &mut ap);
    }
    ExitCode::SUCCESS
}

/// Which config the AP runs on, and the hostapd this daemon started. The one from the boot script
/// is not a child of ours and is stopped by name.
pub struct Ap {
    config: String,
    hostapd: Option<Child>,
}

/// What the host has asked for on this connection, applied as one change.
#[derive(Default)]
struct Wanted {
    ssid: Option<String>,
    country: Option<String>,
    channel: Option<u32>,
    passphrase: Option<String>,
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
            Cmd::Channels => match livi_wifi::listing() {
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
        // The value is the rest of the line, spaces included.
        "set" => match rest.split_once(' ') {
            Some((key, value)) => Cmd::Set(key, value),
            None => Cmd::Unknown(line),
        },
        _ => Cmd::Unknown(head),
    }
}

/// Checks a setting before it reaches the config. A value with a line break in it is refused.
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
            let channel = value
                .parse::<u32>()
                .map_err(|_| "channel must be a number")?;
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

/// The base config with the wanted settings replacing their lines. Everything else stays.
fn config(base: &str, wanted: &Wanted) -> String {
    let mut out = String::new();
    for line in base.lines() {
        let replaced = match setting(line) {
            Some("ssid") => wanted.ssid.is_some(),
            Some("country_code") => wanted.country.is_some(),
            Some("channel" | "hw_mode" | "ht_capab" | "vendor_elements" | "assocresp_elements") => {
                wanted.channel.is_some()
            }
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
    }
    if let Some(ssid) = &wanted.ssid {
        out.push_str(&format!("ssid={ssid}\n"));
    }
    if let Some(passphrase) = &wanted.passphrase {
        out.push_str(&format!("wpa_passphrase={passphrase}\n"));
    }
    out
}

/// The name a config line sets, if it sets one.
fn setting(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    line.split_once('=').map(|(key, _)| key.trim())
}

/// Waits until the radio exists.
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

/// The band a channel sits in, the way hostapd spells it.
fn band(channel: u32) -> &'static str {
    if channel <= 14 { "g" } else { "a" }
}

/// The vendor element a CarPlay access point carries: the vendor id, then the band.
fn apple_ie(channel: u32) -> String {
    let band_bit: u8 = if channel >= 36 { 0x01 } else { 0x02 };
    format!("dd0800a04000000200{:02x}", 0x20 | band_bit)
}

/// Which way the second half of a 40 MHz channel points. The upper member of a pair reaches down.
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
    let base = std::fs::read_to_string(BASE).map_err(|e| format!("{BASE}: {e}"))?;
    if let Some(parent) = std::path::Path::new(LIVE[0]).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // The same settings twice do not restart hostapd.
    let text = config(&base, wanted);
    if running() && std::fs::read_to_string(&ap.config).is_ok_and(|current| current == text) {
        return Ok(());
    }
    let next = if ap.config == LIVE[0] {
        LIVE[1]
    } else {
        LIVE[0]
    };
    std::fs::write(next, text).map_err(|e| format!("{next}: {e}"))?;

    let previous = ap.config.clone();
    stop(ap);
    if let Err(refused) = start(ap, next) {
        // Back to what was running, and to the fallback config if that will not start either.
        stop(ap);
        if start(ap, &previous).is_err() {
            stop(ap);
            let _ = start(ap, BASE);
            ap.config = BASE.to_string();
        }
        return Err(refused);
    }
    ap.config = next.to_string();
    Ok(())
}

/// Makes the running settings the ones the dongle boots with. Written only when it differs.
fn save(ap: &Ap) -> Result<(), String> {
    if ap.config == BASE {
        return Ok(());
    }
    let live = std::fs::read_to_string(&ap.config).map_err(|e| format!("{}: {e}", ap.config))?;
    let base = std::fs::read_to_string(BASE).map_err(|e| format!("{BASE}: {e}"))?;
    let next = config(&base, &settings_of(&live));
    if next == base {
        return Ok(());
    }
    // Written under a second name and renamed over.
    let temp = format!("{BASE}.new");
    std::fs::write(&temp, next).map_err(|e| format!("{temp}: {e}"))?;
    std::fs::rename(&temp, BASE).map_err(|e| format!("{BASE}: {e}"))?;
    let _ = Command::new("sync").status();
    Ok(())
}

/// What a config sets.
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
    // The interface carries no address of its own, l2fwd bridges it onto the host's link.
    let _ = Command::new("ifconfig").args([IFACE, "up"]).status();
    let config = ap.config.clone();
    start(ap, &config)
}

fn off(ap: &mut Ap) {
    stop(ap);
    // Interface down as well.
    let _ = Command::new("ifconfig").args([IFACE, "down"]).status();
}

/// Takes the Bluetooth controller up or down.
fn bluetooth(up: bool) -> Result<(), String> {
    let what = if up { "up" } else { "down" };
    let status = Command::new("hciconfig")
        .args([BT, what])
        .status()
        .map_err(|e| format!("hciconfig: {e}"))?;
    if !status.success() {
        return Err(format!("{BT} would not go {what}"));
    }
    Ok(())
}

/// Whether the controller is up, from the flags hciconfig prints.
fn bt_up() -> bool {
    let Ok(out) = Command::new("hciconfig").arg(BT).output() else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .any(|word| word == "UP")
}

/// The access point's name, from the live config first, then the saved one.
pub fn ap_name() -> Option<String> {
    for file in LIVE.iter().chain(std::iter::once(&BASE)) {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines() {
            if setting(line) == Some("ssid") {
                let value = line.split_once('=').map(|(_, v)| v.trim()).unwrap_or("");
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn status(ap: &Ap) -> String {
    let mut out = String::new();
    out.push_str(if running() {
        "state on\n"
    } else {
        "state off\n"
    });
    out.push_str(if bt_up() { "bt on\n" } else { "bt off\n" });
    if let Ok(mac) = std::fs::read_to_string(format!("/sys/class/net/{IFACE}/address")) {
        out.push_str(&format!("mac {}\n", mac.trim()));
    }
    if let Ok(mac) = std::fs::read_to_string(format!("/sys/class/bluetooth/{BT}/address")) {
        out.push_str(&format!("btmac {}\n", mac.trim()));
    }
    out.push_str(if ap.config == BASE {
        "config fallback\n"
    } else {
        "config host\n"
    });
    if let Ok(text) = std::fs::read_to_string(&ap.config) {
        for line in text.lines() {
            if let Some(key @ ("ssid" | "country_code" | "channel" | "hw_mode")) = setting(line) {
                let value = line.split_once('=').map(|(_, v)| v).unwrap_or("");
                out.push_str(&format!("{key} {value}\n"));
            }
        }
    }
    out.push_str("ok\n");
    out
}

/// Starts hostapd as a child, without `-B`, and waits until the radio reports it is up.
fn start(ap: &mut Ap, config: &str) -> Result<(), String> {
    let _ = std::fs::remove_file(LOG);
    let log = std::fs::File::create(LOG).map_err(|e| format!("{LOG}: {e}"))?;
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
        let text = std::fs::read_to_string(LOG).unwrap_or_default();
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

/// What hostapd objected to: the first line that reads like a complaint, not the tear down.
fn complaint(log: &str) -> String {
    const MARKERS: [&str; 5] = ["not allowed", "Could not", "Unable", "Invalid", "ailed"];
    log.lines()
        .map(str::trim)
        .find(|line| MARKERS.iter().any(|m| line.contains(m)))
        .or_else(|| {
            log.lines()
                .map(str::trim)
                .rev()
                .find(|line| !line.is_empty())
        })
        .unwrap_or("hostapd failed")
        .to_string()
}

fn running() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return false;
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(pairs: &[(&str, &str)]) -> Wanted {
        let mut wanted = Wanted::default();
        for (key, value) in pairs {
            remember(&mut wanted, key, value).unwrap();
        }
        wanted
    }

    const BASE_CONF: &str = "interface=wlan0\n#channel=11\ncountry_code=US\nhw_mode=a\nchannel=36\n\
                             ht_capab=[SHORT-GI-20][SHORT-GI-40][HT40+]\n\
                             ssid=old name\nwpa_passphrase=12345678\nwmm_enabled=1\n";

    #[test]
    fn a_setting_replaces_its_line_and_leaves_the_rest_alone() {
        let out = config(
            BASE_CONF,
            &wanted(&[("ssid", "LIVI Link"), ("channel", "6")]),
        );
        assert!(out.contains("interface=wlan0\n"));
        assert!(out.contains("wmm_enabled=1\n"));
        assert!(out.contains("ssid=LIVI Link\n"));
        assert!(!out.contains("ssid=old name"));
        assert!(out.contains("hw_mode=g\n"));
        assert!(out.contains("channel=6\n"));
        assert!(!out.contains("hw_mode=a\n"));
        // A commented out line is not a setting.
        assert!(out.contains("#channel=11\n"));
        assert!(out.contains("country_code=US\n"));
        assert!(out.contains("wpa_passphrase=12345678\n"));
    }

    #[test]
    fn nothing_wanted_leaves_the_config_as_it_was() {
        assert_eq!(config(BASE_CONF, &Wanted::default()), BASE_CONF);
    }

    #[test]
    fn the_upper_channel_of_a_pair_reaches_down() {
        for (channel, want) in [
            (36, "[HT40+]"),
            (40, "[HT40-]"),
            (44, "[HT40+]"),
            (48, "[HT40-]"),
            (149, "[HT40+]"),
            (153, "[HT40-]"),
            (157, "[HT40+]"),
            (161, "[HT40-]"),
            (6, "[HT40+]"),
            (11, "[HT40-]"),
        ] {
            assert_eq!(ht40(channel), want, "channel {channel}");
        }
    }

    #[test]
    fn a_channel_brings_the_carplay_element_for_its_band() {
        assert_eq!(apple_ie(36), "dd0800a0400000020021");
        let five = config(BASE_CONF, &wanted(&[("channel", "36")]));
        assert!(
            five.contains("vendor_elements=dd0800a0400000020021\n"),
            "{five}"
        );
        assert!(
            five.contains("assocresp_elements=dd0800a0400000020021\n"),
            "{five}"
        );
        let two = config(BASE_CONF, &wanted(&[("channel", "6")]));
        assert!(
            two.contains("vendor_elements=dd0800a0400000020022\n"),
            "{two}"
        );
    }

    #[test]
    fn a_channel_brings_its_own_pair_direction() {
        let out = config(BASE_CONF, &wanted(&[("channel", "48")]));
        assert!(
            out.contains("ht_capab=[SHORT-GI-20][SHORT-GI-40][HT40-]\n"),
            "{out}"
        );
        assert!(!out.contains("[HT40+]"), "{out}");
    }

    #[test]
    fn a_five_gigahertz_channel_picks_the_other_band() {
        let out = config(BASE_CONF, &wanted(&[("channel", "149")]));
        assert!(out.contains("hw_mode=a\n"));
        assert!(out.contains("channel=149\n"));
    }

    #[test]
    fn a_value_that_would_write_its_own_directives_is_refused() {
        let mut w = Wanted::default();
        assert!(remember(&mut w, "ssid", "evil\nchannel=1").is_err());
        assert!(remember(&mut w, "passphrase", "short").is_err());
        assert!(remember(&mut w, "country", "germany").is_err());
        assert!(remember(&mut w, "channel", "0").is_err());
        assert!(remember(&mut w, "channel", "many").is_err());
        assert!(remember(&mut w, "colour", "red").is_err());
        assert!(w.ssid.is_none());
    }

    #[test]
    fn a_country_is_kept_upper_case() {
        let mut w = Wanted::default();
        remember(&mut w, "country", "de").unwrap();
        assert_eq!(w.country.as_deref(), Some("DE"));
    }

    #[test]
    fn a_command_keeps_the_spaces_in_its_value() {
        match command("set ssid My Car (2)") {
            Cmd::Set(key, value) => {
                assert_eq!(key, "ssid");
                assert_eq!(value, "My Car (2)");
            }
            _ => panic!("not a set"),
        }
        assert!(matches!(command("apply"), Cmd::Apply));
        assert!(matches!(command("save"), Cmd::Save));
        assert!(matches!(command("off"), Cmd::Off));
        assert!(matches!(command("bt on"), Cmd::Bt(true)));
        assert!(matches!(command("bt off"), Cmd::Bt(false)));
        assert!(matches!(command("bt sideways"), Cmd::Unknown(_)));
        assert!(matches!(command(""), Cmd::Empty));
        assert!(matches!(command("fly"), Cmd::Unknown("fly")));
        assert!(matches!(command("set ssid"), Cmd::Unknown(_)));
    }

    #[test]
    fn saving_carries_the_whole_state_over() {
        let live = "interface=wlan0\ncountry_code=DE\nhw_mode=g\nchannel=6\n\
                    ssid=Volvo\nwpa_passphrase=geheim12\n";
        let next = config(BASE_CONF, &settings_of(live));
        assert!(next.contains("country_code=DE\n"));
        assert!(next.contains("hw_mode=g\n"));
        assert!(next.contains("channel=6\n"));
        assert!(next.contains("wpa_passphrase=geheim12\n"));
        assert!(next.contains("ssid=Volvo\n"));
        assert!(!next.contains("ssid=old name"));
    }

    #[test]
    fn the_complaint_is_the_reason_and_not_the_tear_down() {
        // Shortened from a real refusal on the dongle.
        let log = "Configuration file: /tmp/livi/hostapd.conf\n\
                   wlan0: interface state UNINITIALIZED->COUNTRY_UPDATE\n\
                   Channel 13 (primary) not allowed for AP mode\n\
                   Could not select hw_mode and channel. (-3)\n\
                   wlan0: AP-DISABLED \n\
                   nl80211: deinit ifname=wlan0 disabled_11b_rates=0\n";
        assert_eq!(
            complaint(log),
            "Channel 13 (primary) not allowed for AP mode"
        );
        assert_eq!(complaint("odd\nstop\n"), "stop");
        assert_eq!(complaint(""), "hostapd failed");
    }
}
