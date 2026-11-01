// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use livi_core_proto::state::{DeviceStatus, DeviceView, Protocol};
use livi_cp::helper_sock::HelperSock;
use livi_session_io::line::LineSock;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SHARED_SOCK: &str = "/tmp/livi-shared.sock";
const IAP_PROFILE_UUID: &str = "00000000-deca-fade-deca-deafdecacafe";
pub const HFP_AG_UUID: &str = "0000111f-0000-1000-8000-00805f9b34fb";
const BT_MAJOR_AUDIO: u32 = 0x04;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ids {
    pub bt_mac: Option<String>,
    pub wifi_mac: Option<String>,
    pub usb_udid: Option<String>,
    pub usb_serial: Option<String>,
    pub instance_id: Option<String>,
    pub ip: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bt_mac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wifi_mac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usb_udid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usb_serial: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_seen: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Present {
    pub bt: bool,
    pub wifi: bool,
    pub usb: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub battery_level: Option<f64>,
    pub battery_charging: Option<bool>,
    pub signal_strength: Option<f64>,
    pub carrier_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    stored: Stored,
    pub present: Present,
    pub current_ip: Option<String>,
    pub status: Status,
}

impl Entry {
    pub fn ids(&self) -> Ids {
        let s = &self.stored;
        Ids {
            bt_mac: s.bt_mac.clone(),
            wifi_mac: s.wifi_mac.clone(),
            usb_udid: s.usb_udid.clone(),
            usb_serial: s.usb_serial.clone(),
            instance_id: s.instance_id.clone(),
            ip: self.current_ip.clone(),
        }
    }

    pub fn id(&self) -> String {
        let s = &self.stored;
        [
            &s.bt_mac,
            &s.usb_udid,
            &s.usb_serial,
            &s.wifi_mac,
            &s.instance_id,
        ]
        .into_iter()
        .flatten()
        .next()
        .cloned()
        .unwrap_or_default()
    }

    pub fn bt_mac(&self) -> Option<&str> {
        self.stored.bt_mac.as_deref()
    }

    pub fn view_protocol(&self) -> Option<Protocol> {
        self.protocol()
    }

    fn protocol(&self) -> Option<Protocol> {
        match self.stored.protocol.as_deref() {
            Some("carplay") => Some(Protocol::Carplay),
            Some("androidauto") => Some(Protocol::Androidauto),
            _ => None,
        }
    }

    fn listed(&self) -> bool {
        let s = &self.stored;
        let stable = s.bt_mac.is_some()
            || s.usb_udid.is_some()
            || s.usb_serial.is_some()
            || s.wifi_mac.is_some()
            || s.instance_id.is_some();
        stable && (s.protocol.is_some() || s.name.is_some())
    }

    fn last_seen(&self) -> f64 {
        self.stored.last_seen.unwrap_or(0.0)
    }
}

pub fn norm_mac(v: &str) -> String {
    let hex: String = v.chars().filter(char::is_ascii_hexdigit).collect();
    if hex.len() != 12 {
        return v.to_lowercase();
    }
    let hex = hex.to_lowercase();
    (0..6)
        .map(|i| &hex[i * 2..i * 2 + 2])
        .collect::<Vec<_>>()
        .join(":")
}

fn real_name(v: Option<&str>) -> Option<String> {
    let name = v?.trim();
    (!name.is_empty() && !name.eq_ignore_ascii_case("android")).then(|| name.to_string())
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

fn same_mac(a: &Option<String>, b: &Option<String>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if norm_mac(a) == norm_mac(b))
}

fn same(a: &Option<String>, b: &Option<String>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a == b)
}

#[derive(Debug, Clone, Default)]
pub struct Seen {
    pub ids: Ids,
    pub name: Option<String>,
    pub model: Option<String>,
    pub protocol: Option<Protocol>,
    pub wired: bool,
}

pub struct Registry {
    path: PathBuf,
    entries: Vec<Entry>,
    load_ok: bool,
}

impl Registry {
    pub fn load(path: &Path) -> Self {
        let mut reg = Self {
            path: path.to_path_buf(),
            entries: Vec::new(),
            load_ok: true,
        };
        let stored: Vec<Stored> = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(list) => list,
                Err(e) => {
                    eprintln!(
                        "[devices] {} does not read ({e}), kept as it is",
                        path.display()
                    );
                    reg.load_ok = false;
                    return reg;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                eprintln!(
                    "[devices] {} does not read ({e}), kept as it is",
                    path.display()
                );
                reg.load_ok = false;
                return reg;
            }
        };
        let mut collapsed = false;
        for mut s in stored {
            s.name = real_name(s.name.as_deref());
            s.bt_mac = s.bt_mac.as_deref().map(norm_mac);
            s.wifi_mac = s.wifi_mac.as_deref().map(norm_mac);
            let e = Entry {
                stored: s,
                ..Default::default()
            };
            let ids = Ids {
                ip: None,
                ..e.ids()
            };
            match reg.find_all(&ids).first() {
                Some(&i) => {
                    reg.entries.push(e);
                    let last = reg.entries.len() - 1;
                    reg.merge(vec![i, last]);
                    collapsed = true;
                }
                None => reg.entries.push(e),
            }
        }
        if collapsed {
            reg.persist();
        }
        reg
    }

    fn persist(&self) {
        if !self.load_ok {
            return;
        }
        let stored: Vec<&Stored> = self
            .entries
            .iter()
            .filter(|e| e.listed())
            .map(|e| &e.stored)
            .collect();
        let Ok(text) = serde_json::to_string_pretty(&stored) else {
            return;
        };
        let tmp = self.path.with_extension("json.tmp");
        let written = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &self.path));
        if let Err(e) = written {
            eprintln!("[devices] {} not saved: {e}", self.path.display());
        }
    }

    fn find_all(&self, ids: &Ids) -> Vec<usize> {
        (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                let s = &e.stored;
                same_mac(&ids.bt_mac, &s.bt_mac)
                    || same_mac(&ids.wifi_mac, &s.wifi_mac)
                    || same(&ids.usb_udid, &s.usb_udid)
                    || same(&ids.usb_serial, &s.usb_serial)
                    || same(&ids.instance_id, &s.instance_id)
                    || same(&ids.ip, &e.current_ip)
            })
            .collect()
    }

    fn merge(&mut self, mut list: Vec<usize>) -> usize {
        let mut first = list.remove(0);
        list.sort_unstable_by(|a, b| b.cmp(a));
        for i in list {
            let o = self.entries.remove(i);
            if i < first {
                first -= 1;
            }
            let p = &mut self.entries[first];
            let (ps, os) = (&mut p.stored, o.stored);
            for (field, other) in [
                (&mut ps.bt_mac, os.bt_mac),
                (&mut ps.wifi_mac, os.wifi_mac),
                (&mut ps.usb_udid, os.usb_udid),
                (&mut ps.usb_serial, os.usb_serial),
                (&mut ps.instance_id, os.instance_id),
                (&mut ps.name, os.name),
                (&mut ps.model, os.model),
                (&mut ps.hostname, os.hostname),
                (&mut ps.protocol, os.protocol),
                (&mut ps.last_transport, os.last_transport),
            ] {
                if field.is_none() {
                    *field = other;
                }
            }
            ps.last_seen = Some(ps.last_seen.unwrap_or(0.0).max(os.last_seen.unwrap_or(0.0)));
            if p.current_ip.is_none() {
                p.current_ip = o.current_ip;
            }
            p.present = Present {
                bt: p.present.bt || o.present.bt,
                wifi: p.present.wifi || o.present.wifi,
                usb: p.present.usb || o.present.usb,
            };
        }
        first
    }

    fn find(&mut self, ids: &Ids) -> Option<usize> {
        let all = self.find_all(ids);
        match all.len() {
            0 => None,
            1 => Some(all[0]),
            _ => Some(self.merge(all)),
        }
    }

    pub fn note_device(&mut self, seen: &Seen) {
        let ids = &seen.ids;
        let i = match self.find(ids) {
            Some(i) => i,
            None => {
                self.entries.push(Entry::default());
                self.entries.len() - 1
            }
        };
        let e = &mut self.entries[i];
        let s = &mut e.stored;
        if let Some(v) = &ids.bt_mac {
            s.bt_mac = Some(norm_mac(v));
        }
        if let Some(v) = &ids.wifi_mac {
            s.wifi_mac = Some(norm_mac(v));
        }
        for (field, v) in [
            (&mut s.usb_udid, &ids.usb_udid),
            (&mut s.usb_serial, &ids.usb_serial),
            (&mut s.instance_id, &ids.instance_id),
            (&mut s.model, &seen.model),
        ] {
            if v.is_some() {
                field.clone_from(v);
            }
        }
        if let Some(name) = real_name(seen.name.as_deref()) {
            s.name = Some(name);
        }
        s.last_transport = Some(if seen.wired { "usb" } else { "wifi" }.into());
        if ids.ip.is_some() {
            e.current_ip.clone_from(&ids.ip);
        }
        let protocol = match seen.protocol {
            Some(Protocol::Androidauto) => "androidauto",
            Some(Protocol::Carplay) => "carplay",
            None => s.protocol.as_deref().unwrap_or("carplay"),
        };
        s.protocol = Some(protocol.into());
        if seen.wired {
            e.present.usb = true;
        } else {
            e.present.wifi = true;
        }
        s.last_seen = Some(now_ms());
        self.persist();
    }

    pub fn note_wifi(&mut self, ids: &Ids, up: bool) {
        let Some(i) = self.find(ids) else { return };
        let e = &mut self.entries[i];
        e.present.wifi = up;
        if up {
            if ids.ip.is_some() {
                e.current_ip.clone_from(&ids.ip);
            }
            e.stored.last_seen = Some(now_ms());
        }
    }

    pub fn clear_presence(&mut self, ids: &Ids) {
        let Some(i) = self.find(ids) else { return };
        let e = &mut self.entries[i];
        e.present = Present::default();
        e.current_ip = None;
    }

    pub fn note_status(&mut self, ids: &Ids, status: &Status) {
        let Some(i) = self.find(ids) else { return };
        let s = &mut self.entries[i].status;
        if status.battery_level.is_some() {
            s.battery_level = status.battery_level;
        }
        if status.battery_charging.is_some() {
            s.battery_charging = status.battery_charging;
        }
        if status.signal_strength.is_some() {
            s.signal_strength = status.signal_strength;
        }
        if status.carrier_name.is_some() {
            s.carrier_name.clone_from(&status.carrier_name);
        }
    }

    pub fn forget(&mut self, id: &str) -> Option<Entry> {
        let nid = norm_mac(id);
        let i = self.entries.iter().position(|e| {
            let s = &e.stored;
            s.bt_mac.as_deref() == Some(nid.as_str())
                || s.usb_udid.as_deref() == Some(id)
                || s.usb_serial.as_deref() == Some(id)
                || s.wifi_mac.as_deref() == Some(nid.as_str())
                || s.instance_id.as_deref() == Some(id)
        })?;
        let removed = self.entries.remove(i);
        self.persist();
        Some(removed)
    }

    pub fn by_id(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id() == id)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

pub type SessionOf<'a> = &'a dyn Fn(&Ids) -> Option<(u32, bool)>;

pub fn views(reg: &Registry, session_of: SessionOf) -> Vec<DeviceView> {
    let mut out: Vec<(DeviceView, f64)> = reg
        .entries()
        .iter()
        .filter(|e| e.listed() && !e.id().is_empty())
        .map(|e| {
            let session = session_of(&e.ids());
            let status = match session {
                Some((_, true)) => DeviceStatus::Active,
                Some((_, false)) => DeviceStatus::Available,
                None if e.present.wifi => DeviceStatus::Available,
                None => DeviceStatus::Offline,
            };
            let view = DeviceView {
                id: e.id(),
                name: e.stored.name.clone(),
                model: e.stored.model.clone(),
                protocol: e.protocol(),
                last_transport: e.stored.last_transport.clone(),
                status,
                battery_level: e.status.battery_level,
                battery_charging: e.status.battery_charging,
                signal_strength: e.status.signal_strength,
                carrier_name: e.status.carrier_name.clone(),
                session: session.map(|(position, _)| position),
            };
            (view, e.last_seen())
        })
        .collect();
    let rank = |s: DeviceStatus| match s {
        DeviceStatus::Active => 0,
        DeviceStatus::Available => 1,
        DeviceStatus::Offline => 2,
    };
    out.sort_by(|(a, a_seen), (b, b_seen)| match (a.session, b.session) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => rank(a.status).cmp(&rank(b.status)).then(
            b_seen
                .partial_cmp(a_seen)
                .unwrap_or(std::cmp::Ordering::Equal),
        ),
    });
    out.into_iter().map(|(v, _)| v).collect()
}

fn wake_uuid(protocol: Option<Protocol>) -> Option<String> {
    match protocol? {
        Protocol::Carplay => Some(IAP_PROFILE_UUID.into()),
        Protocol::Androidauto => Some(HFP_AG_UUID.into()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paging {
    pub auto_connect: bool,
    pub carplay: bool,
    pub android_auto: bool,
}

impl Paging {
    pub fn of(cfg: &livi_core_proto::config::Config) -> Self {
        Self {
            auto_connect: cfg.auto_conn,
            carplay: cfg.wireless_cp_enabled,
            android_auto: cfg.wireless_aa_enabled,
        }
    }

    fn pages(self, protocol: Option<Protocol>) -> bool {
        match protocol {
            Some(Protocol::Carplay) => self.carplay,
            Some(Protocol::Androidauto) => self.android_auto,
            None => self.carplay || self.android_auto,
        }
    }
}

pub fn reconnect_targets(
    reg: &Registry,
    paging: Paging,
    in_session: &dyn Fn(&Ids) -> bool,
    on_cable: &dyn Fn(&str) -> bool,
) -> Vec<(String, Option<String>)> {
    if !paging.auto_connect {
        return Vec::new();
    }
    let mut known: Vec<&Entry> = reg.entries().iter().collect();
    known.sort_by(|a, b| {
        b.last_seen()
            .partial_cmp(&a.last_seen())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    known
        .into_iter()
        .filter(|e| e.stored.protocol.is_some() || e.stored.name.is_some())
        .filter(|e| paging.pages(e.protocol()))
        .filter_map(|e| {
            let mac = e.bt_mac()?;
            (!in_session(&e.ids()) && !on_cable(mac))
                .then(|| (mac.to_uppercase(), wake_uuid(e.protocol())))
        })
        .collect()
}

type Targets = Vec<(String, Option<String>)>;

pub struct Devices {
    pub registry: Registry,
    helper: HelperSock,
    sent: Option<Targets>,
    leaving: bool,
}

impl Devices {
    pub fn new(path: &Path, helper: HelperSock) -> Self {
        Self {
            registry: Registry::load(path),
            helper,
            sent: None,
            leaving: false,
        }
    }

    pub fn page(&mut self, targets: Targets, again: bool) {
        if self.leaving || (!again && self.sent.as_ref() == Some(&targets)) {
            return;
        }
        if !targets.is_empty() {
            let macs: Vec<&str> = targets.iter().map(|(m, _)| m.as_str()).collect();
            println!("[devices] pages {}", macs.join(" "));
        }
        self.sent = Some(targets.clone());
        let helper = self.helper.clone();
        tokio::spawn(async move {
            if let Err(e) = helper.send_reconnect_targets(&targets).await {
                eprintln!("[devices] paging list not sent: {e}");
            }
        });
    }

    pub fn stop_paging(&mut self) -> impl Future<Output = ()> + use<> {
        self.leaving = true;
        self.sent = Some(Vec::new());
        let helper = self.helper.clone();
        async move {
            if let Err(e) = helper.send_reconnect_targets(&[]).await {
                eprintln!("[devices] paging not stopped: {e}");
            }
        }
    }

    pub fn seek(&self, ms: u32, bt_mac: Option<String>) {
        let helper = self.helper.clone();
        tokio::spawn(async move {
            if let Err(e) = helper.seek(ms, bt_mac.as_deref()).await {
                eprintln!("[devices] seek to {ms} ms not sent: {e}");
            }
        });
    }
}

pub async fn shared(line: String, within: Duration) -> Result<Value, String> {
    LineSock::new(SHARED_SOCK, "shared sock")
        .request(&line, within)
        .await
}

pub async fn wake(mac: String, protocol: Option<Protocol>) {
    let line = match wake_uuid(protocol) {
        Some(uuid) => format!("connect {mac} {uuid}"),
        None => format!("connect {mac}"),
    };
    println!("[devices] wake {mac}");
    if let Err(e) = shared(line, Duration::from_secs(32)).await {
        eprintln!("[devices] wake {mac}: {e}");
    }
}

pub async fn unpair(mac: String) {
    let _ = shared(format!("disconnect {mac}"), Duration::from_secs(10)).await;
    if let Err(e) = shared(format!("remove {mac}"), Duration::from_secs(10)).await {
        eprintln!("[devices] unpair {mac}: {e}");
    }
}

pub fn phone_like(class: u64) -> bool {
    class == 0 || (class >> 8) & 0x1f != u64::from(BT_MAJOR_AUDIO)
}

fn class_of(d: &Value) -> u64 {
    d.get("class").and_then(Value::as_u64).unwrap_or(0)
}

async fn paired() -> Option<Vec<Value>> {
    let answer = shared("list_paired".into(), Duration::from_secs(5))
        .await
        .ok()?;
    answer.get("devices").and_then(Value::as_array).cloned()
}

pub async fn connected_phones() -> Vec<String> {
    paired()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|d| d.get("connected").and_then(Value::as_bool) == Some(true))
        .filter(|d| phone_like(class_of(d)))
        .filter_map(|d| d.get("mac").and_then(Value::as_str).map(str::to_string))
        .collect()
}

pub async fn leave_phones() {
    let _ = shared("deauth-ap".into(), Duration::from_secs(2)).await;
    for mac in connected_phones().await {
        println!("[devices] goodbye {mac}");
        let _ = shared(format!("disconnect {mac}"), Duration::from_millis(1500)).await;
    }
}

pub async fn connect_paired(mac: String) {
    let Some(list) = paired().await else { return };
    let phone = list.iter().any(|d| {
        d.get("mac")
            .and_then(Value::as_str)
            .is_some_and(|m| m.eq_ignore_ascii_case(&mac))
            && phone_like(class_of(d))
    });
    if phone {
        return wake(mac, Some(Protocol::Androidauto)).await;
    }
    println!("[devices] connect {mac}");
    if let Err(e) = shared(format!("connect-full {mac}"), Duration::from_secs(32)).await {
        eprintln!("[devices] connect {mac}: {e}");
    }
}

pub async fn nudge_hfp(mac: String) {
    let _ = shared(
        format!("disconnect-profile {mac} {HFP_AG_UUID}"),
        Duration::from_secs(10),
    )
    .await;
    let answer = shared(
        format!("connect {mac} {HFP_AG_UUID}"),
        Duration::from_secs(32),
    )
    .await;
    let outcome = match answer {
        Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(true) => "sent".to_string(),
        Ok(v) => v
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("refused")
            .to_string(),
        Err(e) => e,
    };
    println!("[devices] HFP nudge {mac}: {outcome}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;

    fn seen(bt: &str, wifi: &str, name: &str, wired: bool) -> Seen {
        let opt = |v: &str| (!v.is_empty()).then(|| v.to_string());
        Seen {
            ids: Ids {
                bt_mac: opt(bt),
                wifi_mac: opt(wifi),
                ..Default::default()
            },
            name: opt(name),
            protocol: Some(Protocol::Carplay),
            wired,
            ..Default::default()
        }
    }

    #[test]
    fn macs_are_written_one_way() {
        assert_eq!(norm_mac("AA-BB-CC-DD-EE-FF"), "aa:bb:cc:dd:ee:ff");
        assert_eq!(norm_mac("aabbccddeeff"), "aa:bb:cc:dd:ee:ff");
        assert_eq!(norm_mac("XYZ"), "xyz");
        assert_eq!(real_name(Some(" Android ")), None);
        assert_eq!(real_name(Some("Pixel")), Some("Pixel".into()));
    }

    #[test]
    fn a_phone_is_kept_across_restarts_in_the_old_format() {
        let dir = TempDir::new();
        let path = dir.0.join("devices.json");
        let mut reg = Registry::load(&path);
        reg.note_device(&seen("AA:BB:CC:DD:EE:01", "", "iPhone", true));
        let mut wifi = seen("aa:bb:cc:dd:ee:01", "11:22:33:44:55:66", "", false);
        wifi.ids.ip = Some("10.0.0.5".into());
        reg.note_device(&wifi);
        assert_eq!(reg.entries().len(), 1);

        let text = std::fs::read_to_string(&path).unwrap();
        let file: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(file[0]["btMac"], "aa:bb:cc:dd:ee:01");
        assert_eq!(file[0]["wifiMac"], "11:22:33:44:55:66");
        assert_eq!(file[0]["name"], "iPhone");
        assert_eq!(file[0]["protocol"], "carplay");
        assert_eq!(file[0]["lastTransport"], "wifi");
        assert!(file[0].get("currentIp").is_none());

        let again = Registry::load(&path);
        assert_eq!(again.entries().len(), 1);
        assert_eq!(again.entries()[0].id(), "aa:bb:cc:dd:ee:01");
    }

    #[test]
    fn duplicates_in_the_file_fold_into_one() {
        let dir = TempDir::new();
        let path = dir.0.join("devices.json");
        std::fs::write(
            &path,
            r#"[{"btMac":"AA:BB:CC:DD:EE:01","name":"Android","lastSeen":5},
               {"btMac":"aa:bb:cc:dd:ee:01","wifiMac":"11:22:33:44:55:66","protocol":"androidauto","lastSeen":9}]"#,
        )
        .unwrap();
        let reg = Registry::load(&path);
        assert_eq!(reg.entries().len(), 1);
        let e = &reg.entries()[0];
        assert_eq!(e.stored.wifi_mac.as_deref(), Some("11:22:33:44:55:66"));
        assert_eq!(e.stored.name, None);
        assert_eq!(e.last_seen(), 9.0);
    }

    #[test]
    fn a_broken_file_is_left_alone() {
        let dir = TempDir::new();
        let path = dir.0.join("devices.json");
        std::fs::write(&path, "not json").unwrap();
        let mut reg = Registry::load(&path);
        reg.note_device(&seen("AA:BB:CC:DD:EE:01", "", "iPhone", true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
    }

    #[test]
    fn the_list_shows_sessions_first_then_status_then_recency() {
        let dir = TempDir::new();
        let mut reg = Registry::load(&dir.0.join("devices.json"));
        reg.note_device(&seen("aa:00:00:00:00:01", "", "Old", true));
        reg.note_device(&seen("aa:00:00:00:00:02", "", "Held", true));
        reg.note_device(&seen("aa:00:00:00:00:03", "", "Active", true));
        reg.note_device(&seen(
            "aa:00:00:00:00:04",
            "11:00:00:00:00:04",
            "On the AP",
            false,
        ));
        reg.note_status(
            &Ids {
                bt_mac: Some("AA:00:00:00:00:03".into()),
                ..Default::default()
            },
            &Status {
                battery_level: Some(80.0),
                carrier_name: Some("Net".into()),
                ..Default::default()
            },
        );
        let session_of = |ids: &Ids| match ids.bt_mac.as_deref() {
            Some("aa:00:00:00:00:03") => Some((1, true)),
            Some("aa:00:00:00:00:02") => Some((2, false)),
            _ => None,
        };
        let list = views(&reg, &session_of);
        let names: Vec<_> = list.iter().map(|v| v.name.clone().unwrap()).collect();
        assert_eq!(names, ["Active", "Held", "On the AP", "Old"]);
        assert_eq!(list[0].status, DeviceStatus::Active);
        assert_eq!(list[0].battery_level, Some(80.0));
        assert_eq!(list[0].carrier_name.as_deref(), Some("Net"));
        assert_eq!(
            (list[1].status, list[1].session),
            (DeviceStatus::Available, Some(2))
        );
        assert_eq!(list[2].status, DeviceStatus::Available);
        assert_eq!(list[3].status, DeviceStatus::Offline);
        assert_eq!(list[3].protocol, Some(Protocol::Carplay));
    }

    #[test]
    fn paging_skips_phones_in_a_session_or_on_the_cable() {
        let dir = TempDir::new();
        let mut reg = Registry::load(&dir.0.join("devices.json"));
        reg.note_device(&seen("aa:00:00:00:00:01", "", "One", false));
        reg.note_device(&seen("aa:00:00:00:00:02", "", "Two", false));
        reg.note_device(&seen("aa:00:00:00:00:03", "", "Three", false));
        let mut aa = seen("aa:00:00:00:00:04", "", "Pixel", false);
        aa.protocol = Some(Protocol::Androidauto);
        reg.note_device(&aa);
        let in_session = |ids: &Ids| ids.bt_mac.as_deref() == Some("aa:00:00:00:00:02");
        let on_cable = |mac: &str| mac == "aa:00:00:00:00:03";
        let targets = reconnect_targets(&reg, ALL_ON, &in_session, &on_cable);
        let macs: Vec<_> = targets.iter().map(|(m, _)| m.as_str()).collect();
        assert!(macs.contains(&"AA:00:00:00:00:01") && macs.contains(&"AA:00:00:00:00:04"));
        assert_eq!(macs.len(), 2);
        assert!(targets.contains(&("AA:00:00:00:00:04".into(), Some(HFP_AG_UUID.into()))));
        assert!(targets.contains(&("AA:00:00:00:00:01".into(), Some(IAP_PROFILE_UUID.into()))));
        assert!(
            reconnect_targets(
                &reg,
                Paging {
                    auto_connect: false,
                    ..ALL_ON
                },
                &in_session,
                &on_cable
            )
            .is_empty()
        );
    }

    const ALL_ON: Paging = Paging {
        auto_connect: true,
        carplay: true,
        android_auto: true,
    };

    #[test]
    fn a_phone_is_paged_only_for_a_wireless_protocol_that_is_on() {
        let dir = TempDir::new();
        let mut reg = Registry::load(&dir.0.join("devices.json"));
        reg.note_device(&seen("aa:00:00:00:00:01", "", "iPhone", false));
        let mut aa = seen("aa:00:00:00:00:02", "", "Pixel", false);
        aa.protocol = Some(Protocol::Androidauto);
        reg.note_device(&aa);
        let none = |_: &Ids| false;
        let off = |_: &str| false;
        let macs = |paging| -> Vec<String> {
            reconnect_targets(&reg, paging, &none, &off)
                .into_iter()
                .map(|(m, _)| m)
                .collect()
        };
        assert!(
            macs(Paging {
                carplay: false,
                android_auto: false,
                ..ALL_ON
            })
            .is_empty()
        );
        assert_eq!(
            macs(Paging {
                android_auto: false,
                ..ALL_ON
            }),
            ["AA:00:00:00:00:01"]
        );
        assert_eq!(
            macs(Paging {
                carplay: false,
                ..ALL_ON
            }),
            ["AA:00:00:00:00:02"]
        );
    }

    #[test]
    fn a_forgotten_phone_goes_from_the_list_and_the_file() {
        let dir = TempDir::new();
        let path = dir.0.join("devices.json");
        let mut reg = Registry::load(&path);
        reg.note_device(&seen("aa:00:00:00:00:01", "", "One", true));
        assert!(reg.by_id("aa:00:00:00:00:01").is_some());
        assert!(reg.forget("AA:00:00:00:00:01").is_some());
        assert!(reg.forget("AA:00:00:00:00:01").is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "[]");
    }

    #[test]
    fn an_android_auto_phone_on_the_cable_goes_by_its_serial() {
        let dir = TempDir::new();
        let mut reg = Registry::load(&dir.0.join("devices.json"));
        reg.note_device(&Seen {
            ids: Ids {
                usb_serial: Some("SER1".into()),
                instance_id: Some("inst".into()),
                ..Default::default()
            },
            model: Some("Pixel".into()),
            protocol: Some(Protocol::Androidauto),
            wired: true,
            ..Default::default()
        });
        assert_eq!(reg.entries()[0].id(), "SER1");
        assert!(reg.forget("SER1").is_some());
    }

    #[test]
    fn audio_devices_are_no_phones() {
        assert!(phone_like(0));
        assert!(phone_like(0x5a020c));
        assert!(!phone_like(0x240404));
    }

    #[test]
    fn presence_and_status_need_a_known_phone() {
        let dir = TempDir::new();
        let mut reg = Registry::load(&dir.0.join("devices.json"));
        let unknown = Ids {
            wifi_mac: Some("99:99:99:99:99:99".into()),
            ..Default::default()
        };
        reg.note_wifi(&unknown, true);
        reg.note_status(&unknown, &Status::default());
        reg.clear_presence(&unknown);
        assert!(reg.entries().is_empty());

        reg.note_device(&seen(
            "aa:00:00:00:00:01",
            "11:00:00:00:00:01",
            "One",
            false,
        ));
        let wifi = Ids {
            wifi_mac: Some("11:00:00:00:00:01".into()),
            ip: Some("10.0.0.9".into()),
            ..Default::default()
        };
        reg.note_wifi(&wifi, true);
        assert_eq!(reg.entries()[0].current_ip.as_deref(), Some("10.0.0.9"));
        reg.clear_presence(&wifi);
        assert_eq!(reg.entries()[0].present, Present::default());
    }
}
