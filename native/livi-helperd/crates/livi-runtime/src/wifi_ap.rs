// Wireless-projection AP: owns hostapd + dnsmasq on the dedicated interface.
// Runs as root via `livi-helperd --wifi-ap`.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use livi_wifi::ap_config::{self, HOST_CONF, Standards, Wanted};
use livi_wifi::{Band, Channel, Radio};

const HOSTAPD_CTRL: &str = "/var/run/hostapd";
pub const DNSMASQ_CONF: &str = "/tmp/livi-dnsmasq.conf";
const DNSMASQ_LEASES: &str = "/tmp/livi-dnsmasq.leases";
const HOSTAPD_LOG: &str = "/tmp/livi-hostapd.log";
const NM_UNMANAGED_CONF: &str = "/etc/NetworkManager/conf.d/99-livi-ap-unmanaged.conf";
/// The zone the AP interface was in before the AP.
const FIREWALLD_ZONE: &str = "/run/livi-ap-firewalld-zone";
/// The last frequency and width that carried an access point, kept for a refusal.
const LAST_GOOD: &str = "/tmp/livi-ap-last-good";
/// Where a refused channel lands. Allowed in every regulatory domain, no DFS.
const SAFE_CHANNEL_5: Channel = Channel { band: Band::Ghz5, number: 36 };
const SAFE_CHANNEL_24: Channel = Channel { band: Band::Ghz24, number: 6 };

#[derive(Clone)]
pub struct ApConfig {
    pub iface: String,
    pub ssid: String,
    pub passphrase: String,
    pub channel: Channel,
    /// 20, 40 or 80 (MHz). 80 silently degrades to 40 outside a usable block.
    pub width: u32,
    pub country: String,
    pub ap_ip: String,
}

/// What the radio behind `iface` offers an access point. One that does not describe itself is
/// taken for the 802.11ac radio a host AP always assumed.
fn radio(iface: &str) -> Radio {
    livi_wifi::radio(iface)
        .filter(|r| !r.bands.is_empty())
        .unwrap_or_else(|| Standards { vht: true, he: false }.into())
}

/// For the journal: the bands an access point gets and what it runs on them.
fn described(radio: &Radio) -> String {
    let bands: Vec<String> = radio
        .bands
        .iter()
        .filter(|b| b.band != Band::Ghz6 || radio.ap_sae)
        .map(|b| {
            let mut has = Vec::new();
            for (on, name) in [
                (b.ht.is_some(), "802.11n"),
                (b.vht.is_some(), "802.11ac"),
                (b.he.is_some(), "802.11ax"),
                (b.eht, "802.11be"),
            ] {
                if on {
                    has.push(name);
                }
            }
            format!("{} {}", b.band, has.join("/"))
        })
        .collect();
    let wpa3 = if radio.ap_sae { "WPA3" } else { "WPA2 only" };
    let uapsd = if radio.ap_uapsd { ", power save delivery" } else { "" };
    format!("{}, {wpa3}{uapsd}", bands.join(", "))
}

/// The LIVI Link base on this host's interface, with this AP's name, channel and width and all
/// the radio offers.
fn hostapd_conf(cfg: &ApConfig, radio: &Radio) -> String {
    let wanted = Wanted {
        own_interface: Some(cfg.iface.clone()),
        ssid: Some(cfg.ssid.clone()),
        country: Some(cfg.country.clone()),
        channel: Some(cfg.channel),
        width: Some(cfg.width),
        passphrase: Some(cfg.passphrase.clone()),
        security: Some(ap_config::security_for(radio, cfg.channel.band)),
        ..Wanted::default()
    };
    ap_config::config(ap_config::BASE, &wanted, radio, HOSTAPD_CTRL)
}

fn standard_of(conf: &str) -> &'static str {
    let on = |key: &str| conf.lines().any(|l| l == format!("{key}=1"));
    if on("ieee80211ax") {
        "802.11ax"
    } else if on("ieee80211ac") {
        "802.11ac"
    } else {
        "802.11n"
    }
}

fn write_dnsmasq_conf(cfg: &ApConfig) -> std::io::Result<()> {
    let base = cfg.ap_ip.rsplit_once('.').map(|(b, _)| b.to_string()).unwrap_or_default();
    let conf = format!(
        // port=0 serves DHCP only. A system dnsmasq already holds :53 on many installs.
        "interface={}\nbind-interfaces\nport=0\ndhcp-range={base}.10,{base}.50,255.255.255.0,12h\n\
         dhcp-leasefile={DNSMASQ_LEASES}\n",
        cfg.iface
    );
    std::fs::write(DNSMASQ_CONF, conf)
}

fn run_cmd(cmd: &str, args: &[&str]) {
    let _ = Command::new(crate::sys::tool(cmd))
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn cmd_stdout(cmd: &str, args: &[&str]) -> String {
    Command::new(crate::sys::tool(cmd))
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// NetworkManager profiles rendered into /run vanish once the interface leaves NM;
/// copy them to /etc so client Wi-Fi survives the takeover.
fn persist_nm_profiles() {
    const RUN_DIR: &str = "/run/NetworkManager/system-connections";
    const ETC_DIR: &str = "/etc/NetworkManager/system-connections";
    let listing = cmd_stdout("nmcli", &["-t", "-f", "TYPE,FILENAME", "connection", "show"]);
    let mut copied = false;
    for line in listing.lines() {
        let Some((ty, path)) = line.split_once(':') else { continue };
        if ty != "802-11-wireless" || !path.starts_with(RUN_DIR) {
            continue;
        }
        let Some(name) = path.rsplit('/').next() else { continue };
        let dest = format!("{ETC_DIR}/{name}");
        if std::path::Path::new(&dest).exists() {
            continue;
        }
        run_cmd("install", &["-m", "600", "-o", "root", "-g", "root", path, &dest]);
        copied = true;
    }
    if copied {
        run_cmd("nmcli", &["connection", "reload"]);
    }
}

fn nm_installed() -> bool {
    ["/usr/bin/nmcli", "/bin/nmcli", "/usr/local/bin/nmcli"]
        .iter()
        .any(|p| std::path::Path::new(p).exists())
}

/// firewalld's default zone drops the phone's DHCP request, so the interface sits in `trusted`
/// while the AP is up. Runtime only, nothing permanent.
fn firewalld_open(iface: &str) {
    if cmd_stdout("firewall-cmd", &["--state"]).trim() != "running" {
        return;
    }
    let prior = cmd_stdout("firewall-cmd", &["--get-zone-of-interface", iface]);
    let _ = std::fs::write(FIREWALLD_ZONE, prior.trim());
    run_cmd("firewall-cmd", &["--zone=trusted", &format!("--change-interface={iface}")]);
    println!("[wifi-ap] firewalld: {iface} in the trusted zone while the AP is up");
}

fn firewalld_restore(iface: &str) {
    let Ok(prior) = std::fs::read_to_string(FIREWALLD_ZONE) else { return };
    let _ = std::fs::remove_file(FIREWALLD_ZONE);
    if prior.is_empty() {
        run_cmd("firewall-cmd", &["--zone=trusted", &format!("--remove-interface={iface}")]);
    } else {
        run_cmd(
            "firewall-cmd",
            &[&format!("--zone={prior}"), &format!("--change-interface={iface}")],
        );
    }
}

fn nm_running() -> bool {
    cmd_stdout("systemctl", &["is-active", "NetworkManager"]).trim() == "active"
}

/// The config file NetworkManager reads at start, then the calls for one already running.
pub fn release_iface_from_nm(iface: &str) {
    let content = format!("[keyfile]\nunmanaged-devices=interface-name:{iface}\n");
    if std::fs::read_to_string(NM_UNMANAGED_CONF).unwrap_or_default() != content {
        let _ = std::fs::create_dir_all("/etc/NetworkManager/conf.d");
        let _ = std::fs::write(NM_UNMANAGED_CONF, content);
    }
    // The installer writes the same file, so NetworkManager has it from its first start.
    if nm_installed() && nm_running() {
        run_cmd("nmcli", &["general", "reload"]);
        run_cmd("nmcli", &["device", "set", iface, "managed", "no"]);
        run_cmd("nmcli", &["device", "disconnect", iface]);
    }
    run_cmd("systemctl", &["stop", &format!("wpa_supplicant@{iface}")]);
    run_cmd("rfkill", &["unblock", "wifi"]);
}

/// What the access point is beaconing right now, straight from the kernel.
pub fn status(iface: &str) -> String {
    match livi_wifi::ap_state(iface) {
        Some(ap) => {
            let (band, channel) = (ap.channel.band.setting(), ap.channel.number);
            format!(
                "running true\nssid {}\nband {band}\nchannel {channel}\nwidth {}\n",
                ap.ssid, ap.width
            )
        }
        None => "running false\nssid \nchannel 0\nwidth 0\n".into(),
    }
}

/// Return the interface to NetworkManager and stop the AP.
pub fn unmanaged_iface() -> Option<String> {
    let text = std::fs::read_to_string(NM_UNMANAGED_CONF).ok()?;
    let name = text.split("interface-name:").nth(1)?.split([',', '\n']).next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

pub fn teardown(iface: &str) {
    firewalld_restore(iface);
    run_cmd("pkill", &["-f", &format!("hostapd.*{HOST_CONF}")]);
    run_cmd("pkill", &["-f", &format!("dnsmasq.*{DNSMASQ_CONF}")]);
    run_cmd("ip", &["addr", "flush", "dev", iface, "scope", "global"]);
    keep_awake(iface, false);
    let _ = std::fs::remove_file(NM_UNMANAGED_CONF);
    run_cmd("nmcli", &["general", "reload"]);
    run_cmd("nmcli", &["device", "set", iface, "managed", "yes"]);
    // Let NM bring the client Wi-Fi back on the profiles persist_nm_profiles kept.
    run_cmd("nmcli", &["device", "connect", iface]);
}

fn iface_mac(iface: &str) -> Option<[u8; 6]> {
    let raw = std::fs::read_to_string(format!("/sys/class/net/{iface}/address")).ok()?;
    let mut mac = [0u8; 6];
    for (i, part) in raw.trim().split(':').enumerate().take(6) {
        mac[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}

fn has_link_local(iface: &str) -> bool {
    cmd_stdout("ip", &["-6", "addr", "show", "dev", iface, "scope", "link"]).contains("fe80::")
}

/// CarPlay needs an IPv6 link-local; add the EUI-64 one if the kernel didn't.
fn ensure_link_local(iface: &str) {
    run_cmd("sysctl", &["-qw", &format!("net.ipv6.conf.{iface}.disable_ipv6=0")]);
    run_cmd("sysctl", &["-qw", &format!("net.ipv6.conf.{iface}.addr_gen_mode=0")]);
    if has_link_local(iface) {
        return;
    }
    if let Some(m) = iface_mac(iface) {
        let eui = format!(
            "fe80::{:x}{:02x}:{:02x}ff:fe{:02x}:{:02x}{:02x}",
            m[0] ^ 0x02,
            m[1],
            m[2],
            m[3],
            m[4],
            m[5]
        );
        run_cmd(
            "ip",
            &["-6", "addr", "add", &format!("{eui}/64"), "dev", iface, "scope", "link", "nodad"],
        );
    }
    for _ in 0..8 {
        if has_link_local(iface) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("[wifi-ap] {iface} has no IPv6 link-local, CarPlay needs one");
}

/// The power switches mt76 drivers (MT7921, MT7925, …) expose in debugfs.
const MT76_POWER: [&str; 2] = ["runtime-pm", "deep-sleep"];

fn mt76_debugfs(iface: &str) -> Option<std::path::PathBuf> {
    let phy = std::fs::read_link(format!("/sys/class/net/{iface}/phy80211")).ok()?;
    let dir =
        std::path::Path::new("/sys/kernel/debug/ieee80211").join(phy.file_name()?).join("mt76");
    dir.is_dir().then_some(dir)
}

/// An AP that dozes between frames answers the phone late. Off while LIVI holds the
/// interface, the driver defaults again once it goes back.
fn keep_awake(iface: &str, awake: bool) {
    run_cmd("iw", &["dev", iface, "set", "power_save", if awake { "off" } else { "on" }]);
    let mt76 = mt76_debugfs(iface);
    if let Some(dir) = &mt76 {
        for switch in MT76_POWER {
            let _ = std::fs::write(dir.join(switch), if awake { "0" } else { "1" });
        }
    }
    if awake {
        let extra = if mt76.is_some() { ", mt76 runtime PM and deep sleep off" } else { "" };
        println!("[wifi-ap] {iface} stays awake (power save off{extra})");
    }
}

fn setup_interface(cfg: &ApConfig) {
    run_cmd("iw", &["reg", "set", &cfg.country]);
    run_cmd("ip", &["link", "set", &cfg.iface, "up"]);
    keep_awake(&cfg.iface, true);
    run_cmd("ip", &["addr", "flush", "dev", &cfg.iface, "scope", "global"]);
    run_cmd("ip", &["addr", "add", &format!("{}/24", cfg.ap_ip), "dev", &cfg.iface]);
    ensure_link_local(&cfg.iface);
}

fn hostapd_state(iface: &str) -> String {
    let out = cmd_stdout("hostapd_cli", &["-p", HOSTAPD_CTRL, "-i", iface, "status"]);
    out.lines().find_map(|l| l.strip_prefix("state=")).unwrap_or("").trim().to_string()
}

/// Any process bound to UDP :67 — /proc/net/udp lists ports in hex (0x0043).
fn dhcp_listening() -> bool {
    std::fs::read_to_string("/proc/net/udp")
        .map(|s| {
            s.lines().any(|l| l.split_whitespace().nth(1).is_some_and(|a| a.ends_with(":0043")))
        })
        .unwrap_or(false)
}

/// A hostapd that refused its config is gone at once, the next step need not wait for it.
fn wait_ready(iface: &str, hostapd: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !matches!(hostapd.try_wait(), Ok(None)) {
            return false;
        }
        if hostapd_state(iface) == "ENABLED" && dhcp_listening() && has_link_local(iface) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// The last lines hostapd wrote, for the journal when it did not come up.
fn chrono_free_stamp() -> String {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|t| t.split_whitespace().next().map(|u| format!("uptime {u}s")))
        .unwrap_or_default()
}

fn hostapd_tail() -> String {
    let text = std::fs::read_to_string(HOSTAPD_LOG).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(12);
    lines[from..].join("\n")
}

/// The regulatory domain is requested by `iw reg set` and applied by the kernel some time
/// later. hostapd started in between waits for it in COUNTRY_UPDATE, at boot possibly for
/// longer than the readiness budget.
fn wait_regulatory(country: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if livi_wifi::regulatory_country().as_deref() == Some(country) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    eprintln!(
        "[wifi-ap] regulatory domain {country} not applied after {}s, kernel has {}",
        timeout.as_secs(),
        livi_wifi::regulatory_country().unwrap_or_else(|| "none".into())
    );
}

fn spawn_hostapd() -> std::io::Result<Child> {
    let log = std::fs::OpenOptions::new().create(true).append(true).open(HOSTAPD_LOG)?;
    use std::io::Write as _;
    let _ = writeln!(&log, "--- hostapd start {} ---", chrono_free_stamp());
    Command::new(crate::sys::tool("hostapd"))
        .arg(HOST_CONF)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
}

fn spawn_dnsmasq() -> std::io::Result<Child> {
    Command::new(crate::sys::tool("dnsmasq"))
        .args(["--keep-in-foreground", &format!("--conf-file={DNSMASQ_CONF}")])
        .stdout(Stdio::null())
        .spawn()
}

/// What ran last, as channel and width.
fn last_good() -> Option<(Channel, u32)> {
    let text = std::fs::read_to_string(LAST_GOOD).ok()?;
    let (mhz, width) = text.trim().split_once(' ')?;
    Some((Channel::of_freq(mhz.parse().ok()?)?, width.parse().ok()?))
}

fn after_refusal(cfg: &ApConfig) -> (Channel, u32) {
    match last_good() {
        Some((ch, width)) if ch != cfg.channel => (ch, width),
        _ if cfg.channel.band == Band::Ghz24 => (SAFE_CHANNEL_24, 20),
        _ => (SAFE_CHANNEL_5, 20),
    }
}

/// Bring the AP up and keep it up. Restarts hostapd/dnsmasq when either dies.
pub fn run(cfg: ApConfig) -> ! {
    println!(
        "[wifi-ap] starting — ssid={} channel={} width={}MHz iface={}",
        cfg.ssid, cfg.channel, cfg.width, cfg.iface
    );
    persist_nm_profiles();
    release_iface_from_nm(&cfg.iface);
    firewalld_open(&cfg.iface);
    let _ = std::fs::remove_file(HOSTAPD_LOG);
    let mut cfg = cfg;
    let radio = radio(&cfg.iface);
    println!("[wifi-ap] {} offers {}", cfg.iface, described(&radio));
    // What the radio accepted after refusing more, kept until the channel changes.
    let mut weakened: Option<String> = None;
    loop {
        run_cmd("pkill", &["-f", &format!("hostapd.*{HOST_CONF}")]);
        run_cmd("pkill", &["-f", &format!("dnsmasq.*{DNSMASQ_CONF}")]);
        std::thread::sleep(Duration::from_millis(300));
        setup_interface(&cfg);
        wait_regulatory(&cfg.country, Duration::from_secs(10));
        let conf = weakened.clone().unwrap_or_else(|| hostapd_conf(&cfg, &radio));
        if std::fs::write(HOST_CONF, &conf).is_err() || write_dnsmasq_conf(&cfg).is_err() {
            eprintln!("[wifi-ap] cannot write configs, retrying");
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        let mut dnsmasq = match spawn_dnsmasq() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[wifi-ap] dnsmasq spawn failed: {e}");
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        };
        let mut hostapd = match spawn_hostapd() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[wifi-ap] hostapd spawn failed: {e}");
                let _ = dnsmasq.kill();
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        };
        if wait_ready(&cfg.iface, &mut hostapd, Duration::from_secs(20)) {
            println!(
                "[wifi-ap] AP up — ssid={} ip={} channel={} width={}MHz {}",
                cfg.ssid,
                cfg.ap_ip,
                cfg.channel,
                cfg.width,
                standard_of(&conf)
            );
            let _ = std::io::stdout().flush();
            let _ = std::fs::write(LAST_GOOD, format!("{} {}", cfg.channel.freq_mhz(), cfg.width));
        } else if let Some((next, step)) = ap_config::weaker(&conf) {
            eprintln!("[wifi-ap] hostapd log:\n{}", hostapd_tail());
            eprintln!("[wifi-ap] the radio refused this config, trying {step}");
            weakened = Some(next);
        } else {
            weakened = None;
            let (channel, width) = after_refusal(&cfg);
            eprintln!("[wifi-ap] hostapd log:\n{}", hostapd_tail());
            if (channel, width) == (cfg.channel, cfg.width) {
                eprintln!("[wifi-ap] readiness timeout, restarting stack");
            } else {
                eprintln!(
                    "[wifi-ap] channel {} was refused, going back to {channel} at {width}MHz",
                    cfg.channel
                );
                cfg.channel = channel;
                cfg.width = width;
            }
        }
        // Supervise: leave the pair alone until one of them exits.
        loop {
            if let Ok(Some(st)) = hostapd.try_wait() {
                eprintln!("[wifi-ap] hostapd exited ({st}), restarting");
                break;
            }
            if let Ok(Some(st)) = dnsmasq.try_wait() {
                eprintln!("[wifi-ap] dnsmasq exited ({st}), restarting");
                break;
            }
            if !has_link_local(&cfg.iface) {
                eprintln!("[wifi-ap] {} lost its addresses, setting them again", cfg.iface);
                release_iface_from_nm(&cfg.iface);
                setup_interface(&cfg);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        let _ = hostapd.kill();
        let _ = hostapd.wait();
        let _ = dnsmasq.kill();
        let _ = dnsmasq.wait();
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(channel: u32, width: u32) -> ApConfig {
        ApConfig {
            iface: "wlp104s0".into(),
            ssid: "LIVI".into(),
            passphrase: "x".into(),
            channel: Channel::of_number(channel),
            width,
            country: "DE".into(),
            ap_ip: "10.10.0.1".into(),
        }
    }

    const AC_AX: Standards = Standards { vht: true, he: true };

    fn lines(conf: &str) -> Vec<&str> {
        conf.lines().collect()
    }

    #[test]
    fn the_host_ap_is_the_livi_link_ap_on_its_own_interface() {
        let conf = hostapd_conf(&cfg(36, 40), &AC_AX.into());
        let out = lines(&conf);
        assert!(out.contains(&"interface=wlp104s0"));
        assert!(!out.iter().any(|l| l.starts_with("bridge=")));
        assert!(!out.iter().any(|l| l.starts_with("ieee80211d") || l.starts_with("ieee80211h")));
        for line in [
            "ht_capab=[HT40+][SHORT-GI-20][SHORT-GI-40]",
            "wpa_key_mgmt=WPA-PSK",
            "ieee80211ac=1",
            "vht_oper_chwidth=0",
            "ieee80211ax=1",
            "he_oper_chwidth=0",
            "auth_algs=1",
            "wpa_pairwise=CCMP",
            "ctrl_interface=/var/run/hostapd",
            "ssid=LIVI",
            "country_code=DE",
        ] {
            assert!(out.contains(&line), "{line}");
        }
        assert_eq!(standard_of(&conf), "802.11ax");
    }

    #[test]
    fn eighty_megahertz_takes_the_whole_block() {
        let conf = hostapd_conf(&cfg(36, 80), &AC_AX.into());
        let out = lines(&conf);
        assert!(out.contains(&"vht_oper_chwidth=1"));
        assert!(out.contains(&"vht_oper_centr_freq_seg0_idx=42"));
        assert!(out.contains(&"he_oper_centr_freq_seg0_idx=42"));
    }

    #[test]
    fn eighty_megahertz_outside_a_block_stays_at_forty() {
        let conf = hostapd_conf(&cfg(56, 80), &AC_AX.into());
        let out = lines(&conf);
        assert!(out.contains(&"ht_capab=[HT40-][SHORT-GI-20][SHORT-GI-40]"));
        assert!(out.contains(&"vht_oper_chwidth=0"));
    }

    #[test]
    fn a_radio_without_he_gets_802_11ac() {
        let conf = hostapd_conf(&cfg(36, 40), &Standards { vht: true, he: false }.into());
        assert!(!conf.contains("ieee80211ax"));
        assert_eq!(standard_of(&conf), "802.11ac");
    }

    #[test]
    fn two_point_four_gigahertz_is_802_11n() {
        let conf = hostapd_conf(&cfg(6, 40), &AC_AX.into());
        let out = lines(&conf);
        assert!(out.contains(&"hw_mode=g"));
        assert!(!out.iter().any(|l| l.starts_with("ieee80211ac") || l.starts_with("ieee80211ax")));
        assert_eq!(standard_of(&conf), "802.11n");
    }

    fn sae_radio() -> Radio {
        Radio { ap_sae: true, ap_uapsd: true, ..AC_AX.into() }
    }

    #[test]
    fn a_radio_hostapd_runs_sae_on_offers_wpa3_beside_wpa2() {
        let conf = hostapd_conf(&cfg(36, 40), &sae_radio());
        assert_eq!(livi_wifi::Security::of_hostapd(&conf), livi_wifi::Security::Wpa2Wpa3);
        assert!(lines(&conf).contains(&"uapsd_advertisement_enabled=1"));
    }

    #[test]
    fn the_journal_names_what_the_radio_offers() {
        assert_eq!(
            described(&sae_radio()),
            "2.4 GHz 802.11n, 5 GHz 802.11n/802.11ac/802.11ax, WPA3, power save delivery"
        );
        assert_eq!(
            described(&AC_AX.into()),
            "2.4 GHz 802.11n, 5 GHz 802.11n/802.11ac/802.11ax, WPA2 only"
        );
    }

    #[test]
    fn the_tail_keeps_the_last_twelve_lines() {
        let text: String = (1..=20).map(|i| format!("l{i}\n")).collect();
        let lines: Vec<&str> = text.lines().collect();
        let from = lines.len().saturating_sub(12);
        assert_eq!(lines[from..].first(), Some(&"l9"));
        assert_eq!(lines[from..].len(), 12);
    }
}
