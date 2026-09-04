// Wireless-projection AP: owns hostapd + dnsmasq on the dedicated interface.
// Ported from the python-era wifi_ap.py; runs as root via `livi-helperd --wifi-ap`.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const HOSTAPD_CONF: &str = "/tmp/livi-hostapd.conf";
pub const DNSMASQ_CONF: &str = "/tmp/livi-dnsmasq.conf";
const DNSMASQ_LEASES: &str = "/tmp/livi-dnsmasq.leases";
const HOSTAPD_LOG: &str = "/tmp/livi-hostapd.log";
const NM_UNMANAGED_CONF: &str = "/etc/NetworkManager/conf.d/99-livi-ap-unmanaged.conf";

#[derive(Clone)]
pub struct ApConfig {
    pub iface: String,
    pub ssid: String,
    pub passphrase: String,
    pub channel: u8,
    /// 20, 40 or 80 (MHz). 80 silently degrades to 40 outside a usable block.
    pub width: u8,
    pub country: String,
    pub ap_ip: String,
}

/// Centre segment for 80 MHz VHT. Non-DFS blocks only: UNII-1 → 42, UNII-3 → 155.
fn vht_centre(primary: u8) -> Option<u8> {
    match primary {
        36..=48 => Some(42),
        149..=161 => Some(155),
        _ => None,
    }
}

/// HT40 secondary position: lower channel of each pair uses '+', upper '-'.
fn ht40_secondary(primary: u8) -> char {
    if (primary / 4) % 2 == 1 { '+' } else { '-' }
}

fn radio_section(cfg: &ApConfig) -> String {
    let ch = cfg.channel;
    if ch < 36 {
        // 2.4 GHz: 11n HT20 only.
        return format!("hw_mode=g\nchannel={ch}\nieee80211n=1\n");
    }
    let mut s = format!("hw_mode=a\nchannel={ch}\nieee80211n=1\nieee80211ac=1\n");
    let want80 = cfg.width >= 80 && vht_centre(ch).is_some();
    if cfg.width >= 40 || want80 {
        s.push_str(&format!("ht_capab=[HT40{}]\n", ht40_secondary(ch)));
    }
    if want80 {
        s.push_str(&format!(
            "vht_capab=[SHORT-GI-80]\nvht_oper_chwidth=1\nvht_oper_centr_freq_seg0_idx={}\n",
            vht_centre(ch).unwrap()
        ));
    }
    s
}

fn write_hostapd_conf(cfg: &ApConfig) -> std::io::Result<()> {
    let band_bit: u8 = if cfg.channel >= 36 { 0x01 } else { 0x02 };
    let apple_ie = format!("dd0800a04000000200{:02x}", 0x20 | band_bit);
    let conf = format!(
        "interface={iface}\ndriver=nl80211\nctrl_interface=/var/run/hostapd\nssid={ssid}\n\
         country_code={country}\nieee80211d=1\nieee80211h=0\n{radio}ignore_broadcast_ssid=0\n\
         wmm_enabled=1\nvendor_elements={ie}\nassocresp_elements={ie}\n\
         wpa=2\nwpa_key_mgmt=WPA-PSK\nrsn_pairwise=CCMP\nwpa_passphrase={pass}\n",
        iface = cfg.iface,
        ssid = cfg.ssid,
        country = cfg.country,
        radio = radio_section(cfg),
        ie = apple_ie,
        pass = cfg.passphrase,
    );
    std::fs::write(HOSTAPD_CONF, conf)
}

fn write_dnsmasq_conf(cfg: &ApConfig) -> std::io::Result<()> {
    let base = cfg.ap_ip.rsplit_once('.').map(|(b, _)| b.to_string()).unwrap_or_default();
    let conf = format!(
        "interface={}\nbind-interfaces\ndhcp-range={base}.10,{base}.50,255.255.255.0,12h\n\
         dhcp-leasefile={DNSMASQ_LEASES}\ndomain-needed\nbogus-priv\n",
        cfg.iface
    );
    std::fs::write(DNSMASQ_CONF, conf)
}

fn run_cmd(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

fn cmd_stdout(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
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

/// Keep NM off the AP interface, permanently and for this boot.
fn release_iface_from_nm(iface: &str) {
    let content = format!("[keyfile]\nunmanaged-devices=interface-name:{iface}\n");
    let existing = std::fs::read_to_string(NM_UNMANAGED_CONF).unwrap_or_default();
    if existing != content {
        let _ = std::fs::create_dir_all("/etc/NetworkManager/conf.d");
        let _ = std::fs::write(NM_UNMANAGED_CONF, content);
        run_cmd("nmcli", &["general", "reload"]);
    }
    run_cmd("nmcli", &["device", "set", iface, "managed", "no"]);
    run_cmd("nmcli", &["device", "disconnect", iface]);
    run_cmd("systemctl", &["stop", &format!("wpa_supplicant@{iface}")]);
    run_cmd("rfkill", &["unblock", "wifi"]);
}

/// Return the interface to NetworkManager and stop the AP.
pub fn teardown(iface: &str) {
    run_cmd("pkill", &["-f", &format!("hostapd.*{HOSTAPD_CONF}")]);
    run_cmd("pkill", &["-f", &format!("dnsmasq.*{DNSMASQ_CONF}")]);
    run_cmd("ip", &["addr", "flush", "dev", iface, "scope", "global"]);
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
            m[0] ^ 0x02, m[1], m[2], m[3], m[4], m[5]
        );
        run_cmd("ip", &["-6", "addr", "add", &format!("{eui}/64"), "dev", iface, "scope", "link", "nodad"]);
    }
    for _ in 0..8 {
        if has_link_local(iface) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("[wifi-ap] {iface} has no IPv6 link-local, CarPlay needs one");
}

fn setup_interface(cfg: &ApConfig) {
    run_cmd("iw", &["reg", "set", &cfg.country]);
    run_cmd("ip", &["link", "set", &cfg.iface, "up"]);
    run_cmd("ip", &["addr", "flush", "dev", &cfg.iface, "scope", "global"]);
    run_cmd("ip", &["addr", "add", &format!("{}/24", cfg.ap_ip), "dev", &cfg.iface]);
    ensure_link_local(&cfg.iface);
}

fn hostapd_state(iface: &str) -> String {
    let out = cmd_stdout("hostapd_cli", &["-p", "/var/run/hostapd", "-i", iface, "status"]);
    out.lines()
        .find_map(|l| l.strip_prefix("state="))
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Any process bound to UDP :67 — /proc/net/udp lists ports in hex (0x0043).
fn dhcp_listening() -> bool {
    std::fs::read_to_string("/proc/net/udp")
        .map(|s| s.lines().any(|l| l.split_whitespace().nth(1).is_some_and(|a| a.ends_with(":0043"))))
        .unwrap_or(false)
}

fn wait_ready(iface: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if hostapd_state(iface) == "ENABLED" && dhcp_listening() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

fn spawn_hostapd() -> std::io::Result<Child> {
    let log = std::fs::File::create(HOSTAPD_LOG)?;
    Command::new("hostapd")
        .arg(HOSTAPD_CONF)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
}

fn spawn_dnsmasq() -> std::io::Result<Child> {
    Command::new("dnsmasq")
        .args(["--keep-in-foreground", &format!("--conf-file={DNSMASQ_CONF}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Bring the AP up and keep it up. Restarts hostapd/dnsmasq when either dies.
pub fn run(cfg: ApConfig) -> ! {
    println!(
        "[wifi-ap] starting — ssid={} channel={} width={}MHz iface={}",
        cfg.ssid, cfg.channel, cfg.width, cfg.iface
    );
    persist_nm_profiles();
    release_iface_from_nm(&cfg.iface);
    loop {
        run_cmd("pkill", &["-f", &format!("hostapd.*{HOSTAPD_CONF}")]);
        run_cmd("pkill", &["-f", &format!("dnsmasq.*{DNSMASQ_CONF}")]);
        std::thread::sleep(Duration::from_millis(300));
        setup_interface(&cfg);
        if write_hostapd_conf(&cfg).is_err() || write_dnsmasq_conf(&cfg).is_err() {
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
        if wait_ready(&cfg.iface, Duration::from_secs(20)) {
            println!("[wifi-ap] AP up — ssid={} ip={} channel={} width={}MHz",
                cfg.ssid, cfg.ap_ip, cfg.channel, cfg.width);
            let _ = std::io::stdout().flush();
        } else {
            eprintln!("[wifi-ap] readiness timeout, restarting stack");
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
            std::thread::sleep(Duration::from_secs(2));
        }
        let _ = hostapd.kill();
        let _ = dnsmasq.kill();
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(channel: u8, width: u8) -> ApConfig {
        ApConfig {
            iface: "wlan0".into(),
            ssid: "LIVI".into(),
            passphrase: "x".into(),
            channel,
            width,
            country: "DE".into(),
            ap_ip: "10.10.0.1".into(),
        }
    }

    #[test]
    fn width_80_uses_vht_block() {
        let r = radio_section(&cfg(36, 80));
        assert!(r.contains("ht_capab=[HT40+]"));
        assert!(r.contains("vht_oper_chwidth=1"));
        assert!(r.contains("vht_oper_centr_freq_seg0_idx=42"));
    }

    #[test]
    fn width_80_degrades_outside_blocks() {
        let r = radio_section(&cfg(56, 80));
        assert!(r.contains("ht_capab=[HT40-]"));
        assert!(!r.contains("vht_oper_chwidth"));
    }

    #[test]
    fn width_40_secondary_signs() {
        assert!(radio_section(&cfg(36, 40)).contains("[HT40+]"));
        assert!(radio_section(&cfg(40, 40)).contains("[HT40-]"));
        assert!(radio_section(&cfg(149, 40)).contains("[HT40+]"));
        assert!(radio_section(&cfg(153, 40)).contains("[HT40-]"));
    }

    #[test]
    fn width_20_stays_narrow() {
        let r = radio_section(&cfg(36, 20));
        assert!(!r.contains("ht_capab"));
        assert!(!r.contains("vht"));
    }

    #[test]
    fn band_24ghz_is_ht20_g() {
        let r = radio_section(&cfg(6, 80));
        assert!(r.contains("hw_mode=g"));
        assert!(!r.contains("ht_capab"));
    }
}
