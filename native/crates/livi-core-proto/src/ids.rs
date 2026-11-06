// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ids {
    pub bt_mac: Option<String>,
    pub wifi_mac: Option<String>,
    pub usb_udid: Option<String>,
    pub usb_serial: Option<String>,
    pub instance_id: Option<String>,
    pub controller_id: Option<String>,
    pub ip: Option<String>,
}

impl Ids {
    pub fn any(&self) -> bool {
        [
            &self.bt_mac,
            &self.wifi_mac,
            &self.usb_udid,
            &self.usb_serial,
            &self.instance_id,
            &self.controller_id,
            &self.ip,
        ]
        .into_iter()
        .any(Option::is_some)
    }

    pub fn overlaps(&self, other: &Ids) -> bool {
        let mac = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if same_mac(a, b));
        let same = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if !a.is_empty() && a == b);
        mac(&self.bt_mac, &other.bt_mac)
            || mac(&self.wifi_mac, &other.wifi_mac)
            || same(&self.usb_udid, &other.usb_udid)
            || same(&self.usb_serial, &other.usb_serial)
            || same(&self.instance_id, &other.instance_id)
            || same(&self.controller_id, &other.controller_id)
            || same(&self.ip, &other.ip)
    }

    pub fn merge(&mut self, from: &Ids) {
        let take = |into: &mut Option<String>, v: &Option<String>, mac: bool| {
            if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
                *into = Some(if mac { norm_mac(v) } else { v.clone() });
            }
        };
        take(&mut self.bt_mac, &from.bt_mac, true);
        take(&mut self.wifi_mac, &from.wifi_mac, true);
        take(&mut self.usb_udid, &from.usb_udid, false);
        take(&mut self.usb_serial, &from.usb_serial, false);
        take(&mut self.instance_id, &from.instance_id, false);
        take(&mut self.controller_id, &from.controller_id, false);
        take(&mut self.ip, &from.ip, false);
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

pub fn same_mac(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && norm_mac(a) == norm_mac(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bt(mac: &str) -> Ids {
        Ids {
            bt_mac: Some(mac.into()),
            ..Default::default()
        }
    }

    #[test]
    fn macs_are_written_one_way() {
        assert_eq!(norm_mac("AA-BB-CC-DD-EE-FF"), "aa:bb:cc:dd:ee:ff");
        assert_eq!(norm_mac("aabbccddeeff"), "aa:bb:cc:dd:ee:ff");
        assert_eq!(norm_mac("XYZ"), "xyz");
    }

    #[test]
    fn a_mac_is_the_same_however_it_is_written_but_never_when_empty() {
        assert!(same_mac("AA:BB:CC:DD:EE:FF", "aabbccddeeff"));
        assert!(same_mac("AA:BB", "aa:bb"));
        assert!(!same_mac("", ""));
        assert!(!same_mac("aa:bb", "aa:bc"));
    }

    #[test]
    fn ids_overlap_on_any_shared_identifier() {
        assert!(bt("AA-BB-CC-DD-EE-FF").overlaps(&bt("aa:bb:cc:dd:ee:ff")));
        assert!(!bt("").overlaps(&bt("")));
        let udid = |u: &str| Ids {
            usb_udid: Some(u.into()),
            ..Default::default()
        };
        assert!(udid("u1").overlaps(&udid("u1")));
        assert!(!udid("u1").overlaps(&udid("U1")));
        assert!(!udid("").overlaps(&udid("")));
        assert!(!Ids::default().overlaps(&Ids::default()));
    }

    #[test]
    fn merging_keeps_what_is_known_and_writes_macs_one_way() {
        let mut ids = Ids {
            usb_udid: Some("u1".into()),
            ..Default::default()
        };
        ids.merge(&Ids {
            bt_mac: Some("AABBCCDDEEFF".into()),
            usb_udid: Some(String::new()),
            ..Default::default()
        });
        assert_eq!(ids.bt_mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(ids.usb_udid.as_deref(), Some("u1"));
        assert!(ids.any());
        assert!(!Ids::default().any());
    }
}
