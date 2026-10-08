//! LIVI-Link wifid wire: line-oriented TCP on the dongle's control port. Commands:
//!   channels | status | on | off | apply | save | down | deauth | watch
//!   set <ssid|country|channel|width|passphrase> <value>
//!   bt on | bt off
//!   iap <order>   for the Bluetooth accessory, answered the way iapd answers
//! `on`, `off` and `bt` are kept on the dongle, a boot brings back what was switched last.
//! `down` takes the access point off the air until the next apply or boot, nothing is kept.
//! `deauth` sends every station off, `deauth <count>` says how many.
//! `watch` never answers, it streams `joined <mac>` and `left <mac>` until the client goes.
//! Responses end in `ok\n` or `error <reason>\n`.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub use livi_wifi::ap_config::Standards;
use livi_wifi::ap_config::{self, Wanted, setting};
use livi_wifi::{Channel, Security, listing};

use crate::hostapd::{self, Station};
use crate::radio::{self, Radio};

const RADIO_TRIES: u32 = 40;
const RADIO_POLL: Duration = Duration::from_millis(500);
const START_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(250);
const WATCH_RETRY: Duration = Duration::from_secs(1);

const HOSTAPD: &str = "/usr/sbin/hostapd";
const IFACE: &str = "wlan0";
const BT: &str = "hci0";
const BT_DEV: u16 = 0;
/// Loads the driver and brings hci0 up with btd and iapd, for what a boot left out.
const LIVI_RADIO: &str = "/usr/bin/livi-radio";
const ACCESSORY: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, livi_net::port::ACCESSORY));
const ACCESSORY_WAIT: Duration = Duration::from_secs(3);

pub type OnSave = Box<dyn Fn() + Send + Sync>;

pub struct Ap {
    base: PathBuf,
    live: [PathBuf; 2],
    log: PathBuf,
    config: PathBuf,
    hostapd: Option<Child>,
    on_save: Option<OnSave>,
    standards: Standards,
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
            standards: Standards::default(),
        }
    }

    pub fn with_standards(mut self, standards: Standards) -> Self {
        self.standards = standards;
        self
    }

    pub fn with_on_save(mut self, on_save: OnSave) -> Self {
        self.on_save = Some(on_save);
        self
    }
}

enum Cmd<'a> {
    Channels,
    Status,
    Rates,
    Set(&'a str, &'a str),
    Apply,
    Save,
    Down,
    Deauth,
    Watch,
    On,
    Off,
    Bt(bool),
    Iap(&'a str),
    Empty,
    Unknown(&'a str),
}

pub fn serve<S: std::io::Read + Write>(io: &mut S, ap: &Mutex<Ap>) {
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
            Cmd::Status => status(&held(ap)),
            Cmd::Rates => rates(),
            Cmd::Set(key, value) => match remember(&mut wanted, key, value) {
                Ok(()) => "ok\n".into(),
                Err(e) => format!("error {e}\n"),
            },
            Cmd::Apply => {
                let answer = match apply(&mut held(ap), &wanted) {
                    Ok(()) => "ok\n".into(),
                    Err(e) => format!("error {e}\n"),
                };
                wanted = Wanted::default();
                answer
            }
            Cmd::On => {
                let mut ap = held(ap);
                keep(&ap, Radio::Wifi, true);
                match on(&mut ap) {
                    Ok(()) => "ok\n".into(),
                    Err(e) => format!("error {e}\n"),
                }
            }
            Cmd::Off => {
                let mut ap = held(ap);
                keep(&ap, Radio::Wifi, false);
                deauth();
                off(&mut ap);
                "ok\n".into()
            }
            Cmd::Deauth => format!("deauth {}\nok\n", deauth()),
            Cmd::Watch => {
                watch(reader.get_mut());
                return;
            }
            Cmd::Save => match save(&held(ap)) {
                Ok(()) => "ok\n".into(),
                Err(e) => format!("error {e}\n"),
            },
            Cmd::Down => {
                stop(&mut held(ap));
                println!("[wifid] access point down until the next apply");
                "ok\n".into()
            }
            Cmd::Bt(up) => {
                let ap = held(ap);
                keep(&ap, Radio::Bt, up);
                match bluetooth(up) {
                    Ok(()) => "ok\n".into(),
                    Err(e) => format!("error {e}\n"),
                }
            }
            // Never waits for the access point, an apply can take half a minute
            Cmd::Iap(order) => accessory(ACCESSORY, order),
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
        "rates" => Cmd::Rates,
        "apply" => Cmd::Apply,
        "save" => Cmd::Save,
        "down" => Cmd::Down,
        "deauth" => Cmd::Deauth,
        "watch" => Cmd::Watch,
        "on" => Cmd::On,
        "off" => Cmd::Off,
        "bt" => match rest.trim() {
            "on" => Cmd::Bt(true),
            "off" => Cmd::Bt(false),
            _ => Cmd::Unknown(line),
        },
        "iap" if !rest.trim().is_empty() => Cmd::Iap(rest.trim()),
        "set" => match rest.split_once(' ') {
            Some((key, value)) => Cmd::Set(key, value),
            None => Cmd::Unknown(line),
        },
        _ => Cmd::Unknown(head),
    }
}

fn held(ap: &Mutex<Ap>) -> MutexGuard<'_, Ap> {
    ap.lock().unwrap_or_else(PoisonError::into_inner)
}

fn accessory(at: SocketAddr, order: &str) -> String {
    ask(at, order).unwrap_or_else(|e| format!("error accessory: {e}\n"))
}

fn ask(at: SocketAddr, order: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect_timeout(&at, ACCESSORY_WAIT)?;
    stream.set_read_timeout(Some(ACCESSORY_WAIT))?;
    writeln!(stream, "{order}")?;
    let mut reader = BufReader::new(stream);
    let (mut answer, mut line) = (String::new(), String::new());
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        answer.push_str(&line);
        if line == "ok\n" || line.starts_with("error ") {
            return Ok(answer);
        }
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
            wanted.channel = Some(Channel::of_number(channel));
        }
        "width" => {
            let width = value.parse::<u32>().map_err(|_| "width must be a number")?;
            if ![20, 40, 80].contains(&width) {
                return Err("width must be 20, 40 or 80".into());
            }
            wanted.width = Some(width);
        }
        "passphrase" => {
            if !(8..=63).contains(&value.len()) {
                return Err("passphrase must be 8 to 63 bytes".into());
            }
            wanted.passphrase = Some(value.to_string());
        }
        other => eprintln!("[wifid] setting {other} is not known here, dropped"),
    }
    Ok(())
}

fn ctrl_line() -> String {
    format!("ctrl_interface={}\n", hostapd::CTRL_DIR)
}

/// Without the control socket line no deauth reaches hostapd.
fn with_ctrl(path: &std::path::Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if text.lines().any(|line| setting(line) == Some("ctrl_interface")) {
        return Ok(());
    }
    let gap = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
    std::fs::write(path, format!("{text}{gap}{}", ctrl_line()))
        .map_err(|e| format!("{}: {e}", path.display()))
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

fn apply(ap: &mut Ap, wanted: &Wanted) -> Result<(), String> {
    if !radio::enabled(Radio::Wifi) {
        return Err("wifi is switched off".into());
    }
    await_radio()?;
    let base =
        std::fs::read_to_string(&ap.base).map_err(|e| format!("{}: {e}", ap.base.display()))?;
    if let Some(parent) = ap.live[0].parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = ap_config::config(&base, wanted, &ap.standards.into(), hostapd::CTRL_DIR);
    if running() && std::fs::read_to_string(&ap.config).is_ok_and(|current| current == text) {
        return Ok(());
    }
    let next = if ap.config == ap.live[0] { ap.live[1].clone() } else { ap.live[0].clone() };
    std::fs::write(&next, text).map_err(|e| format!("{}: {e}", next.display()))?;

    let previous = ap.config.clone();
    stop(ap);
    if let Err(refused) = start_weakening(ap, &next) {
        // Removed, so the newest live file is always the one the radio runs on.
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
    let live =
        std::fs::read_to_string(&ap.config).map_err(|e| format!("{}: {e}", ap.config.display()))?;
    let base =
        std::fs::read_to_string(&ap.base).map_err(|e| format!("{}: {e}", ap.base.display()))?;
    let next = ap_config::config(
        &base,
        &ap_config::settings_of(&live),
        &ap.standards.into(),
        hostapd::CTRL_DIR,
    );
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

fn on(ap: &mut Ap) -> Result<(), String> {
    if running() {
        return Ok(());
    }
    driver();
    await_radio()?;
    // The address it kept from its first boot, set while the interface is still down.
    let _ = Command::new(LIVI_RADIO).arg("mac").status();
    let _ = Command::new("ifconfig").args([IFACE, "up"]).status();
    let config = ap.config.clone();
    start_weakening(ap, &config)
}

/// Rewrites `path` to the config the radio finally accepts.
fn start_weakening(ap: &mut Ap, path: &std::path::Path) -> Result<(), String> {
    let refused = match start(ap, path) {
        Ok(()) => return Ok(()),
        Err(refused) => refused,
    };
    let mut text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    while let Some((next, step)) = ap_config::weaker(&text) {
        eprintln!("[wifid] radio refused ({refused}), trying {step}");
        stop(ap);
        std::fs::write(path, &next).map_err(|e| format!("{}: {e}", path.display()))?;
        if start(ap, path).is_ok() {
            return Ok(());
        }
        text = next;
    }
    Err(refused)
}

fn deauth() -> usize {
    if !running() {
        return 0;
    }
    match hostapd::deauth_all(&hostapd::ctrl(IFACE)) {
        Ok(count) => {
            println!("[wifid] deauthenticated {count} station(s)");
            count
        }
        Err(e) => {
            eprintln!("[wifid] deauth: {e}");
            0
        }
    }
}

fn watch<W: Write>(out: &mut W) {
    let ctrl = hostapd::ctrl(IFACE);
    loop {
        let attached = hostapd::watch(&ctrl, |station| {
            let line = match station {
                Some(Station::Joined(mac)) => format!("joined {mac}\n"),
                Some(Station::Left(mac)) => format!("left {mac}\n"),
                None => "\n".to_string(),
            };
            out.write_all(line.as_bytes()).is_ok()
        });
        if attached.is_ok() || out.write_all(b"\n").is_err() {
            return;
        }
        std::thread::sleep(WATCH_RETRY);
    }
}

fn off(ap: &mut Ap) {
    stop(ap);
    let _ = Command::new("ifconfig").args([IFACE, "down"]).status();
}

fn bluetooth(up: bool) -> Result<(), String> {
    if !up {
        // btd first: while a host tunnels, it holds hci0 and hci0 will not go down.
        livi_radio("bt-off", "btd and iapd would not stop")?;
        return livi_btd::hci::down(BT_DEV).map_err(|e| format!("{BT} would not go down: {e}"));
    }
    driver();
    // A boot with Bluetooth off started neither btd nor iapd, they only run once hci0 is up.
    livi_radio("bt", &format!("{BT} would not come up"))
}

fn livi_radio(command: &str, failed: &str) -> Result<(), String> {
    match Command::new(LIVI_RADIO).arg(command).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(failed.into()),
        Err(e) => Err(format!("{LIVI_RADIO}: {e}")),
    }
}

fn keep(ap: &Ap, radio: Radio, on: bool) {
    match radio::set(radio, on) {
        Ok(true) => {
            if let Some(cb) = ap.on_save.as_ref() {
                cb();
            }
        }
        Ok(false) => {}
        Err(e) => eprintln!("[wifid] {}: {e}", radio::PATH),
    }
}

/// With WiFi and Bluetooth both off at boot nothing loaded the driver.
fn driver() {
    let loaded = std::path::Path::new(&format!("/sys/class/net/{IFACE}")).exists()
        || std::path::Path::new(&format!("/sys/class/bluetooth/{BT}")).exists();
    if !loaded {
        let _ = Command::new(LIVI_RADIO).arg("driver").status();
    }
}

fn bt_up() -> bool {
    livi_btd::hci::is_up(BT_DEV)
}

/// The newest live file is the one the radio runs on.
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
    let switch = |radio| if radio::enabled(radio) { "on" } else { "off" };
    out.push_str(&format!(
        "wifi-enabled {}\nbt-enabled {}\n",
        switch(Radio::Wifi),
        switch(Radio::Bt)
    ));
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
        out.push_str(&format!("security {}\n", Security::of_hostapd(&text)));
    }
    let counter = |dir: &str| {
        std::fs::read_to_string(format!("/sys/class/net/{IFACE}/statistics/{dir}_bytes"))
            .ok()
            .map(|s| s.trim().to_string())
    };
    // RX is what the AP received from the phone (down), TX what it sent (up).
    if let Some(bytes) = counter("rx") {
        out.push_str(&format!("downbytes {bytes}\n"));
    }
    if let Some(bytes) = counter("tx") {
        out.push_str(&format!("upbytes {bytes}\n"));
    }
    out.push_str("ok\n");
    out
}

/// Seen from the car: down is phone to car, up is car to phone.
fn rates() -> String {
    match livi_wifi::stations(IFACE).rates {
        Some((down, up)) => format!("downrate {down}\nuprate {up}\nok\n"),
        None => "ok\n".into(),
    }
}

fn start(ap: &mut Ap, config: &std::path::Path) -> Result<(), String> {
    with_ctrl(config)?;
    let _ = std::fs::remove_file(&ap.log);
    let log = std::fs::File::create(&ap.log).map_err(|e| format!("{}: {e}", ap.log.display()))?;
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

    #[test]
    fn a_setting_this_dongle_does_not_know_is_dropped_not_refused() {
        let mut w = Wanted::default();
        assert!(remember(&mut w, "he_bss_color", "12").is_ok());
        assert!(remember(&mut w, "channel", "44").is_ok());
        assert_eq!(w.channel, Some(Channel::of_number(44)));
    }

    #[test]
    fn only_the_three_widths_are_taken() {
        let mut w = Wanted::default();
        assert!(remember(&mut w, "width", "80").is_ok());
        assert_eq!(w.width, Some(80));
        assert!(remember(&mut w, "width", "160").is_err());
        assert!(remember(&mut w, "width", "wide").is_err());
    }

    fn iapd(answer: &'static str) -> (SocketAddr, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let at = listener.local_addr().unwrap();
        let heard = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut order = String::new();
            BufReader::new(&stream).read_line(&mut order).unwrap();
            stream.write_all(answer.as_bytes()).unwrap();
            order
        });
        (at, heard)
    }

    #[test]
    fn down_is_a_command_of_its_own() {
        assert!(matches!(command("down"), Cmd::Down));
    }

    #[test]
    fn the_link_rates_are_a_command_of_their_own() {
        assert!(matches!(command("rates"), Cmd::Rates));
    }

    #[test]
    fn deauth_and_watch_are_commands() {
        assert!(matches!(command("deauth"), Cmd::Deauth));
        assert!(matches!(command("watch"), Cmd::Watch));
    }

    #[test]
    fn an_old_config_gets_the_control_socket_before_hostapd_starts() {
        let path = std::env::temp_dir().join(format!("hostapd-ctrl-{}.conf", std::process::id()));
        std::fs::write(&path, "interface=wlan0\nssid=LIVI").unwrap();
        with_ctrl(&path).unwrap();
        with_ctrl(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "interface=wlan0\nssid=LIVI\nctrl_interface=/tmp/livi/hostapd\n"
        );
        let _ = std::fs::remove_file(&path);
        assert!(with_ctrl(&path).is_err());
    }

    #[test]
    fn an_iap_order_goes_to_the_accessory_and_comes_back_whole() {
        assert!(matches!(command("iap targets aa bb"), Cmd::Iap("targets aa bb")));
        assert!(matches!(command("iap"), Cmd::Unknown("iap")));

        let (at, heard) = iapd("bonds 1\noffered on\ntargets 0\nok\n");
        assert_eq!(accessory(at, "status"), "bonds 1\noffered on\ntargets 0\nok\n");
        assert_eq!(heard.join().unwrap(), "status\n");

        let (at, _) = iapd("error not an address\n");
        assert_eq!(accessory(at, "disconnect x"), "error not an address\n");
    }

    #[test]
    fn an_accessory_that_is_gone_or_hangs_up_is_an_error() {
        let (at, _) = iapd("bonds 1\n");
        assert!(accessory(at, "status").starts_with("error accessory: "));
        let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        assert!(accessory(gone, "on").starts_with("error accessory: "));
    }
}
