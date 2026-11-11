// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use livi_link_ap::radio::{self, Radio};

use crate::http::{S_200, S_400, S_500, T_JSON, err_json, js, json_num};
use crate::{BUILD, VERSION, caps};

pub(crate) fn caps_json() -> String {
    format!(
        r#"{{"flash":{{"mtd":{}}},"led":{}}}"#,
        !caps().flash.mtd.is_empty(),
        caps().led
    )
}

pub(crate) fn bt_json() -> String {
    let bt = caps().bt.as_str();
    let mut mac = read_trim(&format!("/sys/class/bluetooth/{bt}/address"));
    if mac.is_empty() {
        mac = read_trim("/tmp/livi/bt-mac");
    }
    let mut name = read_trim(&format!("/sys/class/bluetooth/{bt}/name"));
    if name.is_empty() {
        name = livi_link_ap::server::ap_name_from(
            std::path::Path::new(HOSTAPD_BASE),
            &[
                std::path::Path::new(HOSTAPD_LIVE[0]),
                std::path::Path::new(HOSTAPD_LIVE[1]),
            ],
        )
        .unwrap_or_default();
    }
    let present = std::path::Path::new(&format!("/sys/class/bluetooth/{bt}")).exists();
    let enabled = radio::enabled(Radio::Bt);
    let state = if !enabled {
        "switched off"
    } else if !present {
        "not present"
    } else if mac.is_empty() {
        "up, MAC unknown"
    } else {
        "up"
    };
    format!(
        r#"{{"enabled":{enabled},"name":"{}","mac":"{}","state":"{}"}}"#,
        js(&name),
        js(&mac),
        js(state)
    )
}

pub(crate) const HOSTAPD_BASE: &str = "/tmp/livi/hostapd.conf.saved";

pub(crate) const HOSTAPD_LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];

pub(crate) fn wifi_json() -> String {
    let iface = caps().wifi_iface.as_str();
    let mut ssid = String::new();
    let mut ch = String::new();
    let mut band = String::new();
    let mut width = 0;
    if let Some(ap) = livi_wifi::ap_state(iface) {
        ssid = ap.ssid;
        ch = ap.channel.number.to_string();
        width = ap.width;
        band = ap.channel.band.to_string();
    }
    if ssid.is_empty() {
        let cfg = livi_link_ap::server::ap_config_from(
            std::path::Path::new(HOSTAPD_BASE),
            &[
                std::path::Path::new(HOSTAPD_LIVE[0]),
                std::path::Path::new(HOSTAPD_LIVE[1]),
            ],
        );
        if let Some(cfg) = cfg {
            for line in cfg.lines() {
                let l = line.trim();
                if let Some(v) = l.strip_prefix("ssid=") {
                    ssid = v.to_string();
                } else if let Some(v) = l.strip_prefix("channel=") {
                    ch = v.to_string();
                    if let Ok(number) = v.parse() {
                        band = livi_wifi::Channel::of_number(number).band.to_string();
                    }
                }
            }
        }
    }
    let mac = read_trim(&format!("/sys/class/net/{iface}/address"));
    let stations = livi_wifi::stations(iface);
    let clients = stations.count;
    let (downrate, uprate) = stations.rates.unwrap_or((0, 0));
    let downbytes = read_trim(&format!("/sys/class/net/{iface}/statistics/rx_bytes"))
        .parse::<u64>()
        .unwrap_or(0);
    let upbytes = read_trim(&format!("/sys/class/net/{iface}/statistics/tx_bytes"))
        .parse::<u64>()
        .unwrap_or(0);
    format!(
        r#"{{"enabled":{},"ssid":"{}","mac":"{}","band":"{}","channel":"{}","width":{},"clients":{},"downrate":{},"uprate":{},"downbytes":{},"upbytes":{}}}"#,
        radio::enabled(Radio::Wifi),
        js(&ssid),
        js(&mac),
        js(&band),
        js(&ch),
        width,
        clients,
        downrate,
        uprate,
        downbytes,
        upbytes
    )
}

pub(crate) fn status_json() -> String {
    let kernel = read_trim("/proc/sys/kernel/osrelease");
    let uptime = fmt_uptime(&read_trim("/proc/uptime"));
    let load = read_trim("/proc/loadavg");
    let mem = fmt_meminfo();
    let mac = read_trim(&format!("/sys/class/net/{}/address", caps().host_iface));
    let mut cp = read_trim(&caps().mfi);
    if cp.is_empty() {
        cp = "not found".into();
    }
    format!(
        r#"{{"model":"{}","target":"{}","version":"{}","build":"{}","kernel":"{}","uptime":"{}","load":"{}","mem":"{}","mac":"{}","cp":"{}"}}"#,
        js(&caps().model),
        js(&caps().target),
        js(VERSION),
        js(BUILD),
        js(&kernel),
        js(&uptime),
        js(&load),
        js(&mem),
        js(&mac),
        js(&cp)
    )
}

pub(crate) fn switch_radio(
    body: Option<&[u8]>,
    radio: Radio,
) -> (&'static str, &'static str, Vec<u8>) {
    let Some(s) = body.and_then(|b| std::str::from_utf8(b).ok()) else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    let on = match json_num(s.trim(), "enabled").as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => return (S_400, T_JSON, err_json("enabled must be 0 or 1")),
    };
    let command = match (radio, on) {
        (Radio::Wifi, true) => "on",
        (Radio::Wifi, false) => "off",
        (Radio::Bt, true) => "bt on",
        (Radio::Bt, false) => "bt off",
    };
    if let Err(e) = wifid(command) {
        return (S_500, T_JSON, err_json(&e));
    }
    let json = match radio {
        Radio::Wifi => wifi_json(),
        Radio::Bt => bt_json(),
    };
    (S_200, T_JSON, json.into_bytes())
}

pub(crate) fn wifid(command: &str) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], livi_net::port::CONTROL));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|e| format!("wifid: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    stream
        .write_all(format!("{command}\n").as_bytes())
        .map_err(|e| format!("wifid: {e}"))?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| format!("wifid: {e}"))?;
    match line.trim() {
        "ok" => Ok(()),
        other => Err(other.strip_prefix("error ").unwrap_or(other).to_string()),
    }
}

pub(crate) fn read_trim(path: &str) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub(crate) fn fmt_uptime(s: &str) -> String {
    let secs = s
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0) as u64;
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

pub(crate) fn fmt_meminfo() -> String {
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    if let Ok(s) = fs::read_to_string("/proc/meminfo") {
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total_kb = rest
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                avail_kb = rest
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
        }
    }
    mem_text(total_kb, avail_kb)
}

pub(crate) fn mem_text(total_kb: u64, avail_kb: u64) -> String {
    let mb = |kb: u64| (kb + 512) / 1024;
    format!(
        "{} MB used / {} MB total",
        mb(total_kb.saturating_sub(avail_kb)),
        mb(total_kb)
    )
}

#[cfg(test)]
mod tests {
    use super::mem_text;
    use crate::flash::raw_mtd;

    #[test]
    fn a_block_node_is_read_back_through_its_character_device() {
        assert_eq!(raw_mtd("mtdblock6"), "/dev/mtd6");
        assert_eq!(raw_mtd("mtdblock12"), "/dev/mtd12");
        assert_eq!(raw_mtd("mtd6"), "/dev/mtd6");
    }

    #[test]
    fn memory_reads_in_whole_megabytes() {
        assert_eq!(mem_text(123_940, 112_276), "11 MB used / 121 MB total");
    }
}
