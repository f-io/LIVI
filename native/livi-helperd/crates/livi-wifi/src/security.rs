//! How a phone authenticates on our access point. Every protocol that tells the phone
//! translates this into its own numbering.

use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Security {
    #[default]
    Wpa2,
    /// WPA2 and WPA3 side by side, the phone picks.
    Wpa2Wpa3,
    Wpa3,
}

/// The hostapd settings a security choice owns.
pub const HOSTAPD_KEYS: [&str; 4] = ["wpa", "wpa_key_mgmt", "ieee80211w", "sae_pwe"];

impl Security {
    pub fn hostapd(self) -> &'static str {
        match self {
            Security::Wpa2 => "wpa=2\nwpa_key_mgmt=WPA-PSK\n",
            Security::Wpa2Wpa3 => "wpa=2\nwpa_key_mgmt=WPA-PSK SAE\nieee80211w=1\nsae_pwe=2\n",
            Security::Wpa3 => "wpa=2\nwpa_key_mgmt=SAE\nieee80211w=2\nsae_pwe=1\n",
        }
    }

    /// What a hostapd config runs.
    pub fn of_hostapd(conf: &str) -> Self {
        let key_mgmt = conf
            .lines()
            .rev()
            .find_map(|line| line.trim().strip_prefix("wpa_key_mgmt="))
            .unwrap_or("");
        let has = |what| key_mgmt.split_whitespace().any(|k| k == what);
        match (has("WPA-PSK"), has("SAE")) {
            (true, true) => Security::Wpa2Wpa3,
            (false, true) => Security::Wpa3,
            _ => Security::Wpa2,
        }
    }

    pub fn of_name(name: &str) -> Option<Self> {
        match name {
            "wpa2" => Some(Security::Wpa2),
            "wpa2-wpa3" => Some(Security::Wpa2Wpa3),
            "wpa3" => Some(Security::Wpa3),
            _ => None,
        }
    }
}

impl fmt::Display for Security {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Security::Wpa2 => "wpa2",
            Security::Wpa2Wpa3 => "wpa2-wpa3",
            Security::Wpa3 => "wpa3",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Security; 3] = [Security::Wpa2, Security::Wpa2Wpa3, Security::Wpa3];

    #[test]
    fn a_config_runs_what_its_lines_say() {
        for security in ALL {
            let conf = format!("ssid=LIVI\n{}wpa_passphrase=x\n", security.hostapd());
            assert_eq!(Security::of_hostapd(&conf), security);
        }
        assert_eq!(Security::of_hostapd("ssid=LIVI\n"), Security::Wpa2);
    }

    #[test]
    fn the_last_key_management_line_counts() {
        let conf = "wpa_key_mgmt=WPA-PSK\nwpa_key_mgmt=SAE\n";
        assert_eq!(Security::of_hostapd(conf), Security::Wpa3);
    }

    #[test]
    fn the_name_on_the_wire_comes_back() {
        for security in ALL {
            assert_eq!(Security::of_name(&security.to_string()), Some(security));
        }
        assert_eq!(Security::of_name("wep"), None);
    }

    #[test]
    fn wpa3_always_protects_management_frames() {
        assert!(Security::Wpa3.hostapd().contains("ieee80211w=2\n"));
        assert!(Security::Wpa2Wpa3.hostapd().contains("ieee80211w=1\n"));
        assert!(!Security::Wpa2.hostapd().contains("ieee80211w"));
    }

    #[test]
    fn every_line_a_choice_writes_is_one_it_owns() {
        for security in ALL {
            for line in security.hostapd().lines() {
                let key = line.split_once('=').unwrap().0;
                assert!(HOSTAPD_KEYS.contains(&key), "{key}");
            }
        }
    }
}
