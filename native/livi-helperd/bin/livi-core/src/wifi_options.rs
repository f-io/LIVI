use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::config::{Config, WifiBand};
use livi_wifi::Band;

use crate::server::Core;

/// Without the radio's own list, only what every regulatory domain allows: no
/// DFS, no UNII-3.
const FALLBACK_24: [u32; 11] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const FALLBACK_5: [u32; 4] = [36, 40, 44, 48];
const ALLOWED_24: [u32; 13] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
const ALLOWED_5: [u32; 9] = [36, 40, 44, 48, 149, 153, 157, 161, 165];
/// The preferred scanning channels, the only ones a phone finds on 6 GHz by itself.
const ALLOWED_6: [u32; 15] = [5, 21, 37, 53, 69, 85, 101, 117, 133, 149, 165, 181, 197, 213, 229];
/// Under this a band is a short range device allowance, not a WLAN one.
const MIN_AP_DBM: f64 = 17.0;
const FALLBACK_COUNTRIES: [&str; 43] = [
    "DE", "AT", "CH", "NL", "BE", "LU", "FR", "GB", "IE", "IT", "ES", "PT", "PL", "CZ", "SK", "HU",
    "RO", "BG", "GR", "HR", "SI", "DK", "SE", "NO", "FI", "IS", "EE", "LV", "LT", "US", "CA", "MX",
    "BR", "AU", "NZ", "JP", "KR", "CN", "IN", "ZA", "AE", "TR", "UA",
];
const TOOL_TIMEOUT: Duration = Duration::from_secs(3);

struct Channel {
    ch: u32,
    freq: u32,
    flags: String,
    dbm: f64,
}

struct Radio {
    country: String,
    channels: Vec<Channel>,
}

fn radio_in(listing: &str, phy: &str) -> Option<Radio> {
    let mut radio = Radio { country: String::new(), channels: Vec::new() };
    let mut current = "";
    for line in listing.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("country") => radio.country = words.next().unwrap_or_default().to_string(),
            Some("phy") => current = words.next().unwrap_or_default(),
            Some("chan") if phy.is_empty() || current == phy => {
                let mut num = || words.next().and_then(|w| w.parse::<u32>().ok());
                let (Some(ch), Some(freq)) = (num(), num()) else { continue };
                let flags = words.next().unwrap_or_default().to_string();
                let dbm = words.next().and_then(|w| w.parse().ok()).unwrap_or(0.0);
                radio.channels.push(Channel { ch, freq, flags, dbm });
            }
            _ => {}
        }
    }
    (!radio.channels.is_empty()).then_some(radio)
}

pub fn band_of(band: WifiBand) -> Band {
    match band {
        WifiBand::Ghz24 => Band::Ghz24,
        WifiBand::Ghz5 => Band::Ghz5,
        WifiBand::Ghz6 => Band::Ghz6,
    }
}

pub fn wifi_band(band: Band) -> WifiBand {
    match band {
        Band::Ghz24 => WifiBand::Ghz24,
        Band::Ghz5 => WifiBand::Ghz5,
        Band::Ghz6 => WifiBand::Ghz6,
    }
}

fn channels_in(radio: Option<&Radio>, band: WifiBand, country: &str) -> Vec<u32> {
    // 6 GHz has no channel every domain allows.
    let (allowed, fallback): (&[u32], &[u32]) = match band {
        WifiBand::Ghz6 => (&ALLOWED_6, &[]),
        WifiBand::Ghz5 => (&ALLOWED_5, &FALLBACK_5),
        WifiBand::Ghz24 => (&ALLOWED_24, &FALLBACK_24),
    };
    let Some(radio) = radio else { return fallback.to_vec() };
    // The driver knows the domain it is on, not the one that was just picked.
    if !country.is_empty()
        && !radio.country.is_empty()
        && !radio.country.eq_ignore_ascii_case(country)
    {
        return fallback.to_vec();
    }
    let band = band_of(band);
    let in_band = |f: u32| livi_wifi::Channel::of_freq(f).is_some_and(|c| c.band == band);
    let chans: BTreeSet<u32> = radio
        .channels
        .iter()
        .filter(|c| c.flags == "ok" && !(c.dbm > 0.0 && c.dbm < MIN_AP_DBM))
        .filter(|c| in_band(c.freq) && allowed.contains(&c.ch))
        .map(|c| c.ch)
        .collect();
    if chans.is_empty() { fallback.to_vec() } else { chans.into_iter().collect() }
}

fn countries_in(dump: &str) -> Vec<String> {
    let codes: BTreeSet<String> = dump
        .lines()
        .filter_map(|l| l.strip_prefix("country ")?.split(':').next())
        .filter(|c| c.len() == 2 && *c != "00")
        .map(str::to_string)
        .collect();
    if codes.is_empty() {
        let mut all: Vec<String> = FALLBACK_COUNTRIES.iter().map(|c| c.to_string()).collect();
        all.sort();
        return all;
    }
    codes.into_iter().collect()
}

fn phy_of(iface: &str) -> String {
    if iface.is_empty() {
        return String::new();
    }
    std::fs::read_to_string(format!("/sys/class/net/{iface}/phy80211/name"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// regdbdump lives in sbin, which a desktop session does not carry in its PATH.
fn tool(name: &str) -> String {
    ["/usr/sbin", "/sbin", "/usr/bin", "/bin"]
        .iter()
        .map(|dir| format!("{dir}/{name}"))
        .find(|p| Path::new(p).exists())
        .unwrap_or_else(|| name.to_string())
}

async fn countries() -> Vec<String> {
    let run = tokio::process::Command::new(tool("regdbdump"))
        .arg("/lib/firmware/regulatory.db")
        .kill_on_drop(true)
        .output();
    let dump = match tokio::time::timeout(TOOL_TIMEOUT, run).await {
        Ok(Ok(out)) => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => String::new(),
    };
    countries_in(&dump)
}

/// 6 GHz takes WPA3, which takes a radio hostapd runs SAE on. The LIVI Link has no 6 GHz.
fn bands_in(radio: Option<&Radio>, country: &str, six_ghz_ap: bool) -> Vec<WifiBand> {
    let mut bands = vec![WifiBand::Ghz24, WifiBand::Ghz5];
    if six_ghz_ap && !channels_in(radio, WifiBand::Ghz6, country).is_empty() {
        bands.push(WifiBand::Ghz6);
    }
    bands
}

async fn options(cfg: &Config) -> (Vec<WifiBand>, Vec<u32>) {
    let phy = phy_of(&cfg.wifi_interface);
    let iface = cfg.wifi_interface.clone();
    let (listing, six_ghz_ap) = tokio::task::spawn_blocking(move || {
        let six = iface != livi_link_host::link::CHOICE
            && livi_wifi::radio(&iface).is_some_and(|r| r.ap_sae);
        (livi_wifi::listing().ok(), six)
    })
    .await
    .unwrap_or((None, false));
    let radio = listing.and_then(|l| radio_in(&l, &phy));
    let bands = bands_in(radio.as_ref(), &cfg.country, six_ghz_ap);
    (bands, channels_in(radio.as_ref(), cfg.wifi_type, &cfg.country))
}

/// What a band change or a radio without the band leaves to correct: the band, else the
/// channel.
fn corrected(cfg: &Config, bands: &[WifiBand], chans: &[u32]) -> Option<serde_json::Value> {
    if !bands.is_empty() && !bands.contains(&cfg.wifi_type) {
        return Some(serde_json::json!({ "wifiType": Band::Ghz5.setting() }));
    }
    snapped(cfg, chans).map(|to| serde_json::json!({ "wifiChannel": to }))
}

/// Where a band change lands when its channel does not exist in the new band.
fn snapped(cfg: &Config, chans: &[u32]) -> Option<u32> {
    let n = cfg.wifi_channel;
    let belongs = match cfg.wifi_type {
        WifiBand::Ghz24 => (1..=14).contains(&n),
        WifiBand::Ghz5 => {
            (n.is_multiple_of(4) && (32..=144).contains(&n))
                || (n % 4 == 1 && (149..=177).contains(&n))
        }
        WifiBand::Ghz6 => n % 4 == 1 && n <= 233,
    };
    if belongs {
        return None;
    }
    let preferred = match cfg.wifi_type {
        WifiBand::Ghz24 => 6,
        WifiBand::Ghz5 => 36,
        WifiBand::Ghz6 => 37,
    };
    Some(if chans.is_empty() || chans.contains(&preferred) { preferred } else { chans[0] })
}

pub async fn follow(core: Arc<Core>) {
    let hub = core.hub.clone();
    let countries = countries().await;
    hub.update(|s| s.system.wifi_countries = countries);
    let mut state = hub.watch();
    let mut last = None;
    loop {
        let cfg = state.borrow_and_update().config.clone();
        let key = (cfg.wifi_type, cfg.country.clone(), cfg.wifi_interface.clone());
        if last.as_ref() != Some(&key) {
            let (bands, chans) = options(&cfg).await;
            let patch = corrected(&cfg, &bands, &chans);
            hub.update(|s| {
                s.system.wifi_bands = bands;
                s.system.wifi_channels = chans;
            });
            if let Some(patch) = patch {
                println!(
                    "[wifi] channel {} on {} does not fit the radio, setting {patch}",
                    cfg.wifi_channel,
                    band_of(cfg.wifi_type)
                );
                if let Err(e) = core.set_config(&patch).await {
                    eprintln!("[wifi] not saved: {e}");
                }
            }
            last = Some(key);
        }
        if state.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "country DE\nphy phy0\nchan 1 2412 ok 20\nchan 6 2437 ok 20\n\
        chan 12 2467 no-ir 20\nchan 36 5180 ok 23\nchan 52 5260 ok 23\nchan 149 5745 ok 14\n\
        phy phy1\nchan 11 2462 ok 20\n";

    #[test]
    fn the_channels_are_those_the_radio_may_send_an_access_point_on() {
        let radio = radio_in(LISTING, "phy0");
        assert_eq!(channels_in(radio.as_ref(), WifiBand::Ghz24, "DE"), [1, 6]);
        // 52 is DFS, 149 is too weak there.
        assert_eq!(channels_in(radio.as_ref(), WifiBand::Ghz5, "de"), [36]);
        let all = radio_in(LISTING, "");
        assert_eq!(channels_in(all.as_ref(), WifiBand::Ghz24, ""), [1, 6, 11]);
    }

    #[test]
    fn without_a_fitting_radio_the_safe_channels_stay() {
        assert_eq!(channels_in(None, WifiBand::Ghz5, "DE"), FALLBACK_5);
        let phy0 = radio_in(LISTING, "phy0");
        assert_eq!(channels_in(phy0.as_ref(), WifiBand::Ghz24, "US"), FALLBACK_24);
        assert!(radio_in(LISTING, "phy9").is_none());
        let only_dfs = radio_in("phy phy0\nchan 52 5260 ok 23\n", "");
        assert_eq!(channels_in(only_dfs.as_ref(), WifiBand::Ghz5, ""), FALLBACK_5);
        assert!(channels_in(None, WifiBand::Ghz6, "DE").is_empty());
    }

    const SIX: &str = "country DE\nphy phy0\nchan 36 5180 ok 23\nchan 33 6115 ok 23\n\
        chan 37 6135 ok 23\nchan 101 6455 disabled 0\n";

    #[test]
    fn six_ghz_offers_its_scanning_channels_and_needs_wpa3() {
        let radio = radio_in(SIX, "phy0");
        assert_eq!(channels_in(radio.as_ref(), WifiBand::Ghz6, "DE"), [37]);
        let all = [WifiBand::Ghz24, WifiBand::Ghz5, WifiBand::Ghz6];
        assert_eq!(bands_in(radio.as_ref(), "DE", true), all);
        assert_eq!(bands_in(radio.as_ref(), "DE", false), all[..2]);
        assert_eq!(bands_in(radio_in(LISTING, "phy0").as_ref(), "DE", true), all[..2]);
    }

    #[test]
    fn a_band_change_takes_a_channel_of_the_new_band() {
        let mut cfg = crate::config_file::defaults();
        (cfg.wifi_type, cfg.wifi_channel) = (WifiBand::Ghz6, 36);
        assert_eq!(snapped(&cfg, &[5, 37]), Some(37));
        assert_eq!(snapped(&cfg, &[5, 21]), Some(5));
        (cfg.wifi_type, cfg.wifi_channel) = (WifiBand::Ghz24, 37);
        assert_eq!(snapped(&cfg, &[1, 6, 11]), Some(6));
        (cfg.wifi_type, cfg.wifi_channel) = (WifiBand::Ghz5, 149);
        assert_eq!(snapped(&cfg, &[36]), None, "a channel of the band stays");
        (cfg.wifi_type, cfg.wifi_channel) = (WifiBand::Ghz6, 37);
        assert_eq!(snapped(&cfg, &[37]), None);
    }

    #[test]
    fn a_radio_without_the_band_takes_5_ghz_first() {
        let mut cfg = crate::config_file::defaults();
        (cfg.wifi_type, cfg.wifi_channel) = (WifiBand::Ghz6, 37);
        let two = [WifiBand::Ghz24, WifiBand::Ghz5];
        assert_eq!(corrected(&cfg, &two, &[37]), Some(serde_json::json!({ "wifiType": "5ghz" })));
        cfg.wifi_type = WifiBand::Ghz5;
        assert_eq!(corrected(&cfg, &two, &[36]), Some(serde_json::json!({ "wifiChannel": 36 })));
        cfg.wifi_channel = 36;
        assert_eq!(corrected(&cfg, &two, &[36]), None);
    }

    #[test]
    fn every_band_maps_to_its_radio_band_and_back() {
        for band in [WifiBand::Ghz24, WifiBand::Ghz5, WifiBand::Ghz6] {
            assert_eq!(wifi_band(band_of(band)), band);
        }
    }

    #[test]
    fn the_countries_come_from_the_database_or_the_fallback() {
        assert_eq!(
            countries_in("country 00: DFS-UNSET\ncountry DE: DFS-ETSI\ncountry AT:"),
            ["AT", "DE"]
        );
        let fallback = countries_in("");
        assert_eq!(fallback.len(), FALLBACK_COUNTRIES.len());
        assert!(fallback.windows(2).all(|w| w[0] < w[1]));
    }
}
