//! The hostapd config of every LIVI access point, the LIVI Link dongle's and the host's alike:
//! one base, rewritten for the wanted channel and width and for what the radio offers, and
//! weakened step by step when the radio refuses it.

use crate::security::{self, Security};
use crate::{Band, BandOffer, Channel, Radio};

/// The base the dongle ships as /etc/hostapd.conf, and the one a host access point starts from.
pub const BASE: &str =
    include_str!("../../../../../scripts/livi-link/common/rootfs/etc/hostapd.conf");

/// The config hostapd runs for a host access point.
pub const HOST_CONF: &str = "/tmp/livi-hostapd.conf";

/// A board's radio as its module is known, for a radio that does not describe itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Standards {
    /// 802.11ac.
    pub vht: bool,
    /// 802.11ax.
    pub he: bool,
}

/// HT40 and both short guard intervals, what every LIVI radio has.
const ASSUMED_HT: u16 = 0x0062;
/// The 80 MHz short guard interval.
const ASSUMED_VHT: u32 = 0x0020;

impl From<Standards> for Radio {
    fn from(standards: Standards) -> Radio {
        let offer = |band, vht: bool, he: bool| BandOffer {
            band,
            ht: Some(ASSUMED_HT),
            vht: vht.then_some(ASSUMED_VHT),
            he: he.then(Vec::new),
            eht: false,
            channels: Vec::new(),
        };
        Radio {
            bands: vec![
                offer(Band::Ghz24, false, false),
                offer(Band::Ghz5, standards.vht, standards.he),
            ],
            ..Radio::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    /// A host radio: its interface replaces the dongle's, and the dongle's bridge goes.
    pub own_interface: Option<String>,
    pub ssid: Option<String>,
    pub country: Option<String>,
    pub channel: Option<Channel>,
    pub width: Option<u32>,
    pub passphrase: Option<String>,
    pub security: Option<Security>,
    /// Some(false) once the radio turned 802.11ac down, so a saved config does not ask again.
    pub ac: Option<bool>,
    /// The same for 802.11ax. None when never asked, so a config saved without it gets it tried
    /// once.
    pub ax: Option<bool>,
}

/// WPA3 where hostapd can run SAE on the radio, alone on 6 GHz where nothing else is allowed.
pub fn security_for(radio: &Radio, band: Band) -> Security {
    match (band, radio.ap_sae) {
        (Band::Ghz6, _) => Security::Wpa3,
        (_, true) => Security::Wpa2Wpa3,
        (_, false) => Security::Wpa2,
    }
}

/// Every line the channel and the radio decide, so neither a base nor a saved config keeps a
/// second one.
const RADIO_KEYS: [&str; 18] = [
    "channel",
    "hw_mode",
    "op_class",
    "ht_capab",
    "vendor_elements",
    "assocresp_elements",
    "ieee80211ac",
    "vht_capab",
    "vht_oper_chwidth",
    "vht_oper_centr_freq_seg0_idx",
    "ieee80211ax",
    "he_oper_chwidth",
    "he_oper_centr_freq_seg0_idx",
    "he_su_beamformer",
    "he_su_beamformee",
    "he_mu_beamformer",
    "he_6ghz_reg_pwr_type",
    "uapsd_advertisement_enabled",
];

pub fn config(base: &str, wanted: &Wanted, radio: &Radio, ctrl_dir: &str) -> String {
    // The base pins vht_oper_centr_freq_seg0_idx to its own channel and hostapd refuses any
    // other, so the channel lines are regenerated for the wanted channel and width.
    let six = wanted.channel.is_some_and(|c| c.band == Band::Ghz6);
    let mut out = String::new();
    for line in base.lines() {
        let replaced = match setting(line) {
            Some("interface") => {
                if let Some(iface) = &wanted.own_interface {
                    out.push_str(&format!("interface={iface}\n"));
                    continue;
                }
                false
            }
            Some("bridge") => wanted.own_interface.is_some(),
            Some("ssid") => wanted.ssid.is_some(),
            Some("country_code") => wanted.country.is_some(),
            Some("ieee80211n") => six,
            Some(key) if RADIO_KEYS.contains(&key) => wanted.channel.is_some(),
            Some("wpa_passphrase") => wanted.passphrase.is_some(),
            Some(key) if security::HOSTAPD_KEYS.contains(&key) => wanted.security.is_some(),
            Some("ctrl_interface") => true,
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
        let width = wanted.width.unwrap_or(40);
        out.push_str(&radio_lines(channel, width, radio.band(channel.band), wanted));
        if radio.ap_uapsd {
            out.push_str("uapsd_advertisement_enabled=1\n");
        }
    }
    if let Some(ssid) = &wanted.ssid {
        out.push_str(&format!("ssid={ssid}\n"));
    }
    if let Some(passphrase) = &wanted.passphrase {
        out.push_str(&format!("wpa_passphrase={passphrase}\n"));
    }
    if let Some(security) = wanted.security {
        out.push_str(security.hostapd());
    }
    out.push_str(&format!("ctrl_interface={ctrl_dir}\n"));
    out
}

fn radio_lines(channel: Channel, width: u32, offer: Option<&BandOffer>, wanted: &Wanted) -> String {
    let ie = apple_ie(channel.band);
    let mut out = format!(
        "hw_mode={}\nchannel={}\nvendor_elements={ie}\nassocresp_elements={ie}\n",
        channel.band.hw_mode(),
        channel.number
    );
    let he = offer.and_then(|o| o.he.as_deref());
    if channel.band == Band::Ghz6 {
        // HE is all there is on 6 GHz, the operating class carries the width.
        let (op_class, centre) = six_ghz(channel, width);
        out.push_str(&format!(
            "op_class={op_class}\nieee80211ax=1\nhe_oper_chwidth={}\n",
            u8::from(width >= 80)
        ));
        if let Some(centre) = centre {
            out.push_str(&format!("he_oper_centr_freq_seg0_idx={centre}\n"));
        }
        out.push_str("he_6ghz_reg_pwr_type=0\n");
        out.push_str(&he_beamforming(he.unwrap_or_default()));
        return out;
    }
    let ht = offer.and_then(|o| o.ht).unwrap_or(ASSUMED_HT);
    out.push_str(&format!("ht_capab={}\n", ht_capab(ht, channel, width)));
    let vht = offer
        .and_then(|o| o.vht)
        .filter(|_| channel.band == Band::Ghz5 && wanted.ac != Some(false));
    let centre = vht_centre(channel).filter(|_| width >= 80 && vht.is_some());
    if let Some(vht) = vht {
        out.push_str(&format!("ieee80211ac=1\nvht_capab={}\n", vht_capab(vht)));
        match centre {
            Some(centre) => out
                .push_str(&format!("vht_oper_chwidth=1\nvht_oper_centr_freq_seg0_idx={centre}\n")),
            None => out.push_str("vht_oper_chwidth=0\n"),
        }
    }
    match (he, wanted.ax) {
        (None, _) => {}
        (Some(_), Some(false)) => out.push_str("ieee80211ax=0\n"),
        (Some(he), _) => {
            match centre {
                Some(centre) => out.push_str(&format!(
                    "ieee80211ax=1\nhe_oper_chwidth=1\nhe_oper_centr_freq_seg0_idx={centre}\n"
                )),
                None => out.push_str("ieee80211ax=1\nhe_oper_chwidth=0\n"),
            }
            out.push_str(&he_beamforming(he));
        }
    }
    out
}

/// The operating class and the centre of a 6 GHz channel at `width` MHz.
fn six_ghz(channel: Channel, width: u32) -> (u32, Option<u32>) {
    let block = |size: u32| (channel.number - 1) / size * size + 1;
    match width {
        0..=20 => (131, None),
        21..=40 => (132, Some(block(8) + 2)),
        _ => (133, Some(block(16) + 6)),
    }
}

/// hostapd's names for the HT capabilities in `bits`.
fn ht_capab(bits: u16, channel: Channel, width: u32) -> String {
    let has = |bit: u16| bits & (1 << bit) != 0;
    let forty = has(1) && width >= 40;
    let mut out = String::new();
    let mut add = |on: bool, token: &str| {
        if on {
            out.push_str(token);
        }
    };
    add(has(0), "[LDPC]");
    add(forty, ht40(channel));
    add(has(4), "[GF]");
    add(has(5), "[SHORT-GI-20]");
    add(has(6) && forty, "[SHORT-GI-40]");
    add(has(7), "[TX-STBC]");
    add((bits >> 8) & 3 == 1, "[RX-STBC1]");
    add((bits >> 8) & 3 == 2, "[RX-STBC12]");
    add((bits >> 8) & 3 == 3, "[RX-STBC123]");
    add(has(11), "[MAX-AMSDU-7935]");
    add(has(12) && forty && channel.band == Band::Ghz24, "[DSSS_CCK-40]");
    out
}

/// hostapd's names for the VHT capabilities in `bits`.
fn vht_capab(bits: u32) -> String {
    let field = |shift: u32, mask: u32| (bits >> shift) & mask;
    let mut out = String::new();
    let mut add = |on: bool, token: String| {
        if on {
            out.push_str(&token);
        }
    };
    add(field(0, 3) == 1, "[MAX-MPDU-7991]".into());
    add(field(0, 3) == 2, "[MAX-MPDU-11454]".into());
    add(field(2, 3) == 1, "[VHT160]".into());
    add(field(2, 3) == 2, "[VHT160-80PLUS80]".into());
    add(field(4, 1) == 1, "[RXLDPC]".into());
    add(field(5, 1) == 1, "[SHORT-GI-80]".into());
    add(field(6, 1) == 1, "[SHORT-GI-160]".into());
    add(field(7, 1) == 1, "[TX-STBC-2BY1]".into());
    let rx_stbc = ["", "[RX-STBC-1]", "[RX-STBC-12]", "[RX-STBC-123]", "[RX-STBC-1234]"];
    add(field(8, 7) > 0, rx_stbc.get(field(8, 7) as usize).copied().unwrap_or("").into());
    add(field(11, 1) == 1, "[SU-BEAMFORMER]".into());
    add(field(12, 1) == 1, "[SU-BEAMFORMEE]".into());
    add(field(13, 7) > 0, format!("[BF-ANTENNA-{}]", field(13, 7) + 1));
    add(field(16, 7) > 0, format!("[SOUNDING-DIMENSION-{}]", field(16, 7) + 1));
    add(field(19, 1) == 1, "[MU-BEAMFORMER]".into());
    add(field(20, 1) == 1, "[MU-BEAMFORMEE]".into());
    add(field(21, 1) == 1, "[VHT-TXOP-PS]".into());
    add(field(22, 1) == 1, "[HTC-VHT]".into());
    add(field(23, 7) > 0, format!("[MAX-A-MPDU-LEN-EXP{}]", field(23, 7)));
    add(field(26, 3) == 2, "[VHT-LINK-ADAPT2]".into());
    add(field(26, 3) == 3, "[VHT-LINK-ADAPT3]".into());
    add(field(28, 1) == 1, "[RX-ANTENNA-PATTERN]".into());
    add(field(29, 1) == 1, "[TX-ANTENNA-PATTERN]".into());
    out
}

/// The beamforming roles in the HE PHY capabilities, bits 31 to 33.
fn he_beamforming(phy: &[u8]) -> String {
    let bit = |n: usize| phy.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0);
    let mut out = String::new();
    for (n, key) in [(31, "he_su_beamformer"), (32, "he_su_beamformee"), (33, "he_mu_beamformer")] {
        if bit(n) {
            out.push_str(&format!("{key}=1\n"));
        }
    }
    out
}

pub fn setting(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    line.split_once('=').map(|(key, _)| key.trim())
}

pub fn apple_ie(band: Band) -> String {
    let band_bit: u8 = if band == Band::Ghz24 { 0x02 } else { 0x01 };
    format!("dd0800a04000000200{:02x}", 0x20 | band_bit)
}

fn vht_centre(channel: Channel) -> Option<u32> {
    match (channel.band, channel.number) {
        (Band::Ghz5, 36..=48) => Some(42),
        (Band::Ghz5, 149..=161) => Some(155),
        _ => None,
    }
}

fn ht40(channel: Channel) -> &'static str {
    let n = channel.number;
    let up = if channel.band == Band::Ghz24 { n <= 7 } else { (n / 4) % 2 == 1 };
    if up { "[HT40+]" } else { "[HT40-]" }
}

fn value_in(text: &str, key: &str) -> Option<String> {
    text.lines()
        .rfind(|line| setting(line) == Some(key))
        .and_then(|line| line.split_once('='))
        .map(|(_, value)| value.trim().to_string())
}

/// 6 GHz names its channels by the operating class.
fn six_ghz_class(text: &str) -> Option<u32> {
    value_in(text, "op_class")?.parse().ok().filter(|c| (131..=137).contains(c))
}

fn channel_in(text: &str) -> Option<Channel> {
    let number = value_in(text, "channel")?.parse().ok()?;
    Some(match six_ghz_class(text) {
        Some(_) => Channel::new(Band::Ghz6, number),
        None => Channel::of_number(number),
    })
}

pub fn settings_of(text: &str) -> Wanted {
    let value = |key: &str| value_in(text, key);
    let width = match six_ghz_class(text) {
        Some(131) => 20,
        Some(132) => 40,
        Some(_) => 80,
        None if value("vht_oper_chwidth").as_deref() == Some("1") => 80,
        None if value("ht_capab").is_some_and(|c| c.contains("[HT40")) => 40,
        None => 20,
    };
    Wanted {
        own_interface: None,
        ssid: value("ssid"),
        country: value("country_code"),
        channel: channel_in(text),
        width: Some(width),
        passphrase: value("wpa_passphrase"),
        security: Some(Security::of_hostapd(text)),
        ac: Some(value("ieee80211ac").as_deref() == Some("1")),
        ax: value("ieee80211ax").map(|ax| ax == "1"),
    }
}

const BASIC_HT: [&str; 4] = ["[HT40+]", "[HT40-]", "[SHORT-GI-20]", "[SHORT-GI-40]"];
const EXTRAS: [&str; 4] =
    ["he_su_beamformer", "he_su_beamformee", "he_mu_beamformer", "uapsd_advertisement_enabled"];

/// The next config to try after the radio refused `config`, and what it gives up.
/// `ieee80211ax=0` stays in, so a saved config remembers that the radio refused it.
pub fn weaker(config: &str) -> Option<(String, &'static str)> {
    let value = |key: &str| value_in(config, key);
    let channel = channel_in(config)?;
    let mut lines: Vec<String> = config.lines().map(str::to_string).collect();
    let mut set = |key: &str, to: Option<&str>| {
        let had = lines.iter().any(|line| setting(line) == Some(key));
        lines.retain(|line| setting(line) != Some(key));
        if let Some(to) = to.filter(|_| had) {
            lines.push(format!("{key}={to}"));
        }
    };
    let ht = value("ht_capab").unwrap_or_default();
    let basic_ht: String = BASIC_HT.iter().filter(|t| ht.contains(**t)).copied().collect();
    let vht = value("vht_capab");
    let security = Security::of_hostapd(config);
    let step = if ht != basic_ht
        || vht.as_deref().is_some_and(|v| v != "[SHORT-GI-80]")
        || EXTRAS.iter().any(|key| value(key).is_some())
    {
        set("ht_capab", Some(basic_ht.as_str()));
        set("vht_capab", Some("[SHORT-GI-80]"));
        for key in EXTRAS {
            set(key, None);
        }
        "the basic capabilities"
    } else if channel.band == Band::Ghz6 {
        return None;
    } else if security != Security::Wpa2 {
        set("wpa_key_mgmt", Some("WPA-PSK"));
        set("ieee80211w", None);
        set("sae_pwe", None);
        "WPA2 only"
    } else if value("vht_oper_chwidth").as_deref() == Some("1") {
        set("vht_oper_chwidth", Some("0"));
        set("vht_oper_centr_freq_seg0_idx", None);
        set("vht_capab", None);
        set("he_oper_chwidth", Some("0"));
        set("he_oper_centr_freq_seg0_idx", None);
        "40 MHz"
    } else if value("ieee80211ax").as_deref() == Some("1") {
        set("ieee80211ax", Some("0"));
        set("he_oper_chwidth", None);
        set("he_oper_centr_freq_seg0_idx", None);
        "802.11ac"
    } else if value("ieee80211ac").as_deref() == Some("1") {
        for key in ["ieee80211ac", "vht_capab", "vht_oper_chwidth", "vht_oper_centr_freq_seg0_idx"]
        {
            set(key, None);
        }
        "802.11n"
    } else if ht.contains("[HT40") {
        set("ht_capab", Some("[SHORT-GI-20]"));
        "20 MHz"
    } else if channel.band != Band::Ghz24 {
        let ie = apple_ie(Band::Ghz24);
        set("hw_mode", Some("g"));
        set("channel", Some("6"));
        set("ht_capab", Some("[SHORT-GI-20]"));
        set("vendor_elements", Some(ie.as_str()));
        set("assocresp_elements", Some(ie.as_str()));
        "2.4 GHz channel 6"
    } else {
        return None;
    };
    Some((lines.join("\n") + "\n", step))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: &str = "/tmp/livi/hostapd";

    const TEST_BASE: &str = "interface=wlan0\nssid=LIVI-Link\nhw_mode=a\nchannel=36\nieee80211ac=1\n\
        ht_capab=[HT40+][SHORT-GI-20][SHORT-GI-40]\nvht_capab=[SHORT-GI-80]\n\
        vht_oper_chwidth=1\nvht_oper_centr_freq_seg0_idx=42\nwpa_passphrase=livilink\n";

    const N_ONLY: Standards = Standards { vht: false, he: false };
    const AC_ONLY: Standards = Standards { vht: true, he: false };
    const AC_AX: Standards = Standards { vht: true, he: true };

    fn wanted(channel: u32, width: Option<u32>) -> Wanted {
        Wanted { channel: Some(Channel::of_number(channel)), width, ..Wanted::default() }
    }

    fn cfg(base: &str, wanted: &Wanted, standards: Standards) -> String {
        config(base, wanted, &standards.into(), CTRL)
    }

    fn lines(config: &str) -> Vec<&str> {
        config.lines().collect()
    }

    #[test]
    fn eighty_megahertz_gets_its_centre_where_the_block_needs_no_dfs() {
        let out = cfg(TEST_BASE, &wanted(149, Some(80)), AC_AX);
        let out = lines(&out);
        assert!(out.contains(&"vht_oper_chwidth=1"));
        assert!(out.contains(&"vht_oper_centr_freq_seg0_idx=155"));
        assert!(out.contains(&"ht_capab=[HT40+][SHORT-GI-20][SHORT-GI-40]"));
        assert!(!out.contains(&"vht_oper_centr_freq_seg0_idx=42"));
    }

    #[test]
    fn eighty_megahertz_on_a_dfs_channel_stays_at_forty() {
        let out = cfg(TEST_BASE, &wanted(100, Some(80)), AC_AX);
        let out = lines(&out);
        assert!(out.contains(&"vht_oper_chwidth=0"));
        assert!(!out.iter().any(|l| l.starts_with("vht_oper_centr_freq_seg0_idx")));
    }

    #[test]
    fn a_host_that_names_no_width_gets_forty() {
        let out = cfg(TEST_BASE, &wanted(36, None), AC_AX);
        let out = lines(&out);
        assert!(out.contains(&"vht_oper_chwidth=0"));
        assert!(out.contains(&"ht_capab=[HT40+][SHORT-GI-20][SHORT-GI-40]"));
    }

    #[test]
    fn twenty_megahertz_drops_the_secondary_channel() {
        let out = cfg(TEST_BASE, &wanted(36, Some(20)), AC_AX);
        let out = lines(&out);
        assert!(out.contains(&"ht_capab=[SHORT-GI-20]"));
        assert!(out.contains(&"vht_oper_chwidth=0"));
    }

    #[test]
    fn a_saved_config_keeps_its_width() {
        for width in [20, 40, 80] {
            let live = cfg(TEST_BASE, &wanted(36, Some(width)), AC_AX);
            assert_eq!(settings_of(&live).width, Some(width));
        }
    }

    #[test]
    fn a_base_that_lost_its_vht_lines_gets_them_back() {
        let worn = "interface=wlan0\nssid=LIVI\nieee80211n=1\nchannel=36\n\
            ht_capab=[HT40+][SHORT-GI-20][SHORT-GI-40]\n";
        let out = cfg(worn, &wanted(36, Some(80)), AC_AX);
        let out = lines(&out);
        assert!(out.contains(&"ieee80211ac=1"));
        assert!(out.contains(&"vht_oper_chwidth=1"));
        assert!(out.contains(&"vht_oper_centr_freq_seg0_idx=42"));
    }

    #[test]
    fn a_radio_without_vht_never_gets_it() {
        let out = cfg(TEST_BASE, &wanted(36, Some(80)), N_ONLY);
        let out = lines(&out);
        assert!(!out.iter().any(|l| l.starts_with("ieee80211a") || l.starts_with("vht_")));
        assert!(!out.iter().any(|l| l.starts_with("he_")));
        assert!(out.contains(&"ht_capab=[HT40+][SHORT-GI-20][SHORT-GI-40]"));
    }

    #[test]
    fn a_radio_without_he_gets_802_11ac_but_never_802_11ax() {
        let worn = "interface=wlan0\nchannel=36\nieee80211ax=1\nhe_oper_chwidth=1\n";
        let out = cfg(worn, &wanted(36, Some(80)), AC_ONLY);
        let out = lines(&out);
        assert!(out.contains(&"ieee80211ac=1"));
        assert!(out.contains(&"vht_oper_chwidth=1"));
        assert!(!out.iter().any(|l| l.starts_with("ieee80211ax") || l.starts_with("he_")));
    }

    #[test]
    fn eighty_megahertz_asks_for_802_11ax_on_the_same_block() {
        let out = cfg(TEST_BASE, &wanted(149, Some(80)), AC_AX);
        let out = lines(&out);
        assert!(out.contains(&"ieee80211ax=1"));
        assert!(out.contains(&"he_oper_chwidth=1"));
        assert!(out.contains(&"he_oper_centr_freq_seg0_idx=155"));
    }

    #[test]
    fn forty_megahertz_asks_for_802_11ax_without_a_centre() {
        let out = cfg(TEST_BASE, &wanted(36, Some(40)), AC_AX);
        let out = lines(&out);
        assert!(out.contains(&"ieee80211ax=1"));
        assert!(out.contains(&"he_oper_chwidth=0"));
        assert!(!out.iter().any(|l| l.starts_with("he_oper_centr_freq_seg0_idx")));
    }

    #[test]
    fn a_refusing_radio_is_asked_for_less_step_by_step() {
        let mut text = cfg(TEST_BASE, &wanted(36, Some(80)), AC_AX);
        let mut steps = Vec::new();
        while let Some((next, step)) = weaker(&text) {
            steps.push(step);
            text = next;
        }
        assert_eq!(steps, ["40 MHz", "802.11ac", "802.11n", "20 MHz", "2.4 GHz channel 6"]);
        let ie = format!("vendor_elements={}", apple_ie(Band::Ghz24));
        let out = lines(&text);
        assert!(out.contains(&"hw_mode=g"));
        assert!(out.contains(&"channel=6"));
        assert!(out.contains(&"ht_capab=[SHORT-GI-20]"));
        assert!(out.contains(&ie.as_str()));
        assert!(!out.iter().any(|l| l.starts_with("ieee80211ac") || l.starts_with("vht_")));
        assert!(!out.iter().any(|l| l.starts_with("he_") || *l == "ieee80211ax=1"));
        assert!(out.contains(&"wpa_passphrase=livilink"));
    }

    #[test]
    fn forty_megahertz_keeps_802_11ax_until_the_radio_refuses_that_too() {
        let live = cfg(TEST_BASE, &wanted(36, Some(80)), AC_AX);
        let (forty, _) = weaker(&live).unwrap();
        let forty = lines(&forty);
        assert!(forty.contains(&"ieee80211ax=1"));
        assert!(forty.contains(&"he_oper_chwidth=0"));
        assert!(!forty.iter().any(|l| l.starts_with("he_oper_centr_freq_seg0_idx")));
    }

    #[test]
    fn a_config_the_radio_ran_without_ax_is_saved_without_asking_again() {
        let live = cfg(TEST_BASE, &wanted(36, Some(80)), AC_AX);
        let (forty, _) = weaker(&live).unwrap();
        let (ac, _) = weaker(&forty).unwrap();
        let saved = cfg(TEST_BASE, &settings_of(&ac), AC_AX);
        let saved = lines(&saved);
        assert!(saved.contains(&"ieee80211ax=0"));
        assert!(!saved.iter().any(|l| l.starts_with("he_")));
        assert!(saved.contains(&"ieee80211ac=1"));
    }

    #[test]
    fn a_config_saved_before_802_11ax_gets_it_tried() {
        let before = cfg(TEST_BASE, &Wanted { ax: Some(false), ..wanted(36, Some(80)) }, AC_AX)
            .replace("ieee80211ax=0\n", "");
        let out = cfg(TEST_BASE, &settings_of(&before), AC_AX);
        assert!(lines(&out).contains(&"ieee80211ax=1"));
    }

    #[test]
    fn a_config_the_radio_ran_without_ac_is_saved_without_it() {
        let live = cfg(TEST_BASE, &wanted(36, Some(80)), AC_AX);
        let (forty, _) = weaker(&live).unwrap();
        let (ac, _) = weaker(&forty).unwrap();
        let (n, _) = weaker(&ac).unwrap();
        let saved = cfg(TEST_BASE, &settings_of(&n), AC_AX);
        let saved = lines(&saved);
        assert!(!saved.iter().any(|l| l.starts_with("ieee80211ac") || l.starts_with("vht_")));
        assert!(!saved.iter().any(|l| l.starts_with("he_") || *l == "ieee80211ax=1"));
        assert!(saved.contains(&"ht_capab=[HT40+][SHORT-GI-20][SHORT-GI-40]"));
    }

    #[test]
    fn every_config_opens_the_control_socket_once() {
        let worn = format!("{TEST_BASE}ctrl_interface=/var/run/hostapd\n");
        let out = cfg(&worn, &wanted(36, Some(80)), AC_AX);
        assert_eq!(out.matches("ctrl_interface=").count(), 1);
        assert!(out.contains("ctrl_interface=/tmp/livi/hostapd\n"));
    }

    #[test]
    fn a_host_radio_takes_the_dongle_base_on_its_own_interface() {
        let host = Wanted {
            own_interface: Some("wlp104s0".into()),
            ssid: Some("LIVI".into()),
            country: Some("DE".into()),
            passphrase: Some("secret".into()),
            ..wanted(36, Some(40))
        };
        let out = config(BASE, &host, &AC_AX.into(), "/var/run/hostapd");
        let out = lines(&out);
        assert_eq!(out.iter().filter(|l| l.starts_with("interface=")).count(), 1);
        assert!(out.contains(&"interface=wlp104s0"));
        assert!(!out.iter().any(|l| l.starts_with("bridge=")));
        for line in ["auth_algs=1", "wpa_pairwise=CCMP", "rsn_pairwise=CCMP", "wmm_enabled=1"] {
            assert!(out.contains(&line), "{line}");
        }
        assert!(out.contains(&"ieee80211ax=1"));
        assert!(out.contains(&"ssid=LIVI"));
        assert!(out.contains(&"wpa_passphrase=secret"));
        assert!(!out.contains(&"ssid=LIVI-Link"));
        assert!(!out.contains(&"wpa_passphrase=livilink"));
        assert!(out.contains(&"ctrl_interface=/var/run/hostapd"));
    }

    #[test]
    fn the_dongle_keeps_its_own_interface_and_bridge() {
        let out = config(BASE, &wanted(36, Some(80)), &AC_AX.into(), CTRL);
        let out = lines(&out);
        assert!(out.contains(&"interface=wlan0"));
        assert!(out.contains(&"bridge=br0"));
    }

    #[test]
    fn the_security_asked_for_replaces_the_base_and_is_read_back() {
        for security in [Security::Wpa2, Security::Wpa2Wpa3, Security::Wpa3] {
            let asked = Wanted { security: Some(security), ..wanted(36, Some(40)) };
            let out = config(BASE, &asked, &AC_AX.into(), CTRL);
            assert_eq!(out.matches("wpa_key_mgmt=").count(), 1);
            assert_eq!(out.matches("\nwpa=").count(), 1);
            assert_eq!(settings_of(&out).security, Some(security));
            assert_eq!(Security::of_hostapd(&out), security);
        }
    }

    #[test]
    fn without_a_security_wish_the_base_keeps_its_own() {
        let out = config(BASE, &wanted(36, Some(40)), &AC_AX.into(), CTRL);
        let out = lines(&out);
        assert!(out.contains(&"wpa_key_mgmt=WPA-PSK"));
        assert!(!out.iter().any(|l| l.starts_with("ieee80211w") || l.starts_with("sae_pwe")));
    }

    /// The MT7925 as it describes itself on 5 and 6 GHz.
    fn mt7925() -> Radio {
        let offer = |band, ht, vht, channels: &[u32]| BandOffer {
            band,
            ht,
            vht,
            he: Some(vec![0x0c, 0x20, 0xce, 0x12, 0x00, 0x00, 0xa0, 0x00, 0x00, 0x0c, 0x00]),
            eht: true,
            channels: channels.to_vec(),
        };
        Radio {
            bands: vec![
                offer(Band::Ghz24, Some(0x09ff), None, &[1, 6, 11]),
                offer(Band::Ghz5, Some(0x09ff), Some(0x339071f6), &[36, 40, 44, 48]),
                offer(Band::Ghz6, None, None, &[5, 21, 37]),
            ],
            ap_uapsd: true,
            ap_sae: true,
            sae_offload: false,
        }
    }

    #[test]
    fn a_radio_gets_every_capability_it_names() {
        let asked = Wanted { security: Some(Security::Wpa2Wpa3), ..wanted(36, Some(40)) };
        let out = config(BASE, &asked, &mt7925(), CTRL);
        let out = lines(&out);
        for line in [
            "ht_capab=[LDPC][HT40+][GF][SHORT-GI-20][SHORT-GI-40][TX-STBC][RX-STBC1][MAX-AMSDU-7935]",
            "vht_capab=[MAX-MPDU-11454][VHT160][RXLDPC][SHORT-GI-80][SHORT-GI-160][TX-STBC-2BY1]\
             [RX-STBC-1][SU-BEAMFORMEE][BF-ANTENNA-4][MU-BEAMFORMEE][MAX-A-MPDU-LEN-EXP7]\
             [RX-ANTENNA-PATTERN][TX-ANTENNA-PATTERN]",
            "ieee80211ax=1",
            "uapsd_advertisement_enabled=1",
            "wpa_key_mgmt=WPA-PSK SAE",
            "ieee80211w=1",
        ] {
            assert!(out.contains(&line), "{line}");
        }
        assert!(!out.iter().any(|l| l.starts_with("he_su_") || l.starts_with("he_mu_")));
    }

    #[test]
    fn he_runs_on_2_4_ghz_too() {
        let out = config(BASE, &wanted(6, Some(20)), &mt7925(), CTRL);
        let out = lines(&out);
        assert!(out.contains(&"hw_mode=g"));
        assert!(out.contains(&"ieee80211ax=1"));
        assert!(
            out.contains(&"ht_capab=[LDPC][GF][SHORT-GI-20][TX-STBC][RX-STBC1][MAX-AMSDU-7935]")
        );
        assert!(!out.iter().any(|l| l.starts_with("ieee80211ac") || l.starts_with("vht_")));
    }

    #[test]
    fn a_beamformer_says_so() {
        let mut radio = mt7925();
        radio.bands[1].he = Some(vec![0, 0, 0, 0x80, 0x03]);
        let out = config(BASE, &wanted(36, Some(40)), &radio, CTRL);
        for key in ["he_su_beamformer=1", "he_su_beamformee=1", "he_mu_beamformer=1"] {
            assert!(lines(&out).contains(&key), "{key}");
        }
    }

    fn six(number: u32, width: u32) -> Wanted {
        Wanted {
            channel: Some(Channel::new(Band::Ghz6, number)),
            width: Some(width),
            security: Some(Security::Wpa3),
            ..Wanted::default()
        }
    }

    #[test]
    fn six_ghz_runs_he_alone_with_wpa3() {
        let out = config(BASE, &six(37, 80), &mt7925(), CTRL);
        let lines = lines(&out);
        for line in [
            "hw_mode=a",
            "channel=37",
            "op_class=133",
            "ieee80211ax=1",
            "he_oper_chwidth=1",
            "he_oper_centr_freq_seg0_idx=39",
            "he_6ghz_reg_pwr_type=0",
            "wpa_key_mgmt=SAE",
            "ieee80211w=2",
            "sae_pwe=1",
        ] {
            assert!(lines.contains(&line), "{line}");
        }
        assert!(!lines.iter().any(|l| l.starts_with("ieee80211n")
            || l.starts_with("ht_capab")
            || l.starts_with("ieee80211ac")
            || l.starts_with("vht_")));
        let back = settings_of(&out);
        assert_eq!(back.channel, Some(Channel::new(Band::Ghz6, 37)));
        assert_eq!(back.width, Some(80));
        assert_eq!(back.security, Some(Security::Wpa3));
    }

    #[test]
    fn a_6_ghz_width_picks_its_class_and_centre() {
        let out = config(BASE, &six(37, 40), &mt7925(), CTRL);
        assert!(lines(&out).contains(&"op_class=132"));
        assert!(lines(&out).contains(&"he_oper_centr_freq_seg0_idx=35"));
        assert!(lines(&out).contains(&"he_oper_chwidth=0"));
        let out = config(BASE, &six(37, 20), &mt7925(), CTRL);
        assert!(lines(&out).contains(&"op_class=131"));
        assert!(!out.contains("he_oper_centr_freq_seg0_idx"));
        assert_eq!(settings_of(&out).width, Some(20));
    }

    #[test]
    fn a_refusal_gives_up_the_extras_first_and_wpa3_next() {
        let asked = Wanted { security: Some(Security::Wpa2Wpa3), ..wanted(36, Some(80)) };
        let mut text = config(BASE, &asked, &mt7925(), CTRL);
        let mut steps = Vec::new();
        while let Some((next, step)) = weaker(&text) {
            steps.push(step);
            text = next;
        }
        assert_eq!(
            steps,
            [
                "the basic capabilities",
                "WPA2 only",
                "40 MHz",
                "802.11ac",
                "802.11n",
                "20 MHz",
                "2.4 GHz channel 6"
            ]
        );
        assert_eq!(Security::of_hostapd(&text), Security::Wpa2);
        assert!(!text.contains("uapsd_advertisement_enabled"));
    }

    #[test]
    fn a_refused_6_ghz_channel_is_left_to_the_channel_fallback() {
        let text = config(BASE, &six(37, 80), &mt7925(), CTRL);
        let (basic, step) = weaker(&text).unwrap();
        assert_eq!(step, "the basic capabilities");
        assert!(!basic.contains("uapsd_advertisement_enabled"));
        assert_eq!(weaker(&basic), None);
    }

    #[test]
    fn wpa3_needs_a_radio_hostapd_runs_sae_on() {
        let radio = mt7925();
        assert_eq!(security_for(&radio, Band::Ghz5), Security::Wpa2Wpa3);
        assert_eq!(security_for(&radio, Band::Ghz6), Security::Wpa3);
        let firmware_sme = Radio { ap_sae: false, ..radio };
        assert_eq!(security_for(&firmware_sme, Band::Ghz24), Security::Wpa2);
    }

    #[test]
    fn a_board_known_only_by_its_module_keeps_the_old_capabilities() {
        let out = config(BASE, &wanted(36, Some(80)), &AC_AX.into(), CTRL);
        let out = lines(&out);
        assert!(out.contains(&"ht_capab=[HT40+][SHORT-GI-20][SHORT-GI-40]"));
        assert!(out.contains(&"vht_capab=[SHORT-GI-80]"));
        assert!(!out.iter().any(|l| l.starts_with("uapsd") || l.starts_with("he_su")));
    }
}
