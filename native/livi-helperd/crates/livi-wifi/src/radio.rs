//! What a radio offers an access point, as nl80211 describes it.

use crate::{Band, Channel};

const BAND_ATTR_HT_CAPA: u16 = 4;
const BAND_ATTR_VHT_CAPA: u16 = 8;
const BAND_ATTR_IFTYPE_DATA: u16 = 9;
const IFTYPE_ATTR_IFTYPES: u16 = 1;
const IFTYPE_ATTR_HE_CAP_PHY: u16 = 3;
const IFTYPE_ATTR_EHT_CAP_PHY: u16 = 9;
const ATTR_SUPPORT_AP_UAPSD: u16 = 130;
const ATTR_DEVICE_AP_SME: u16 = 141;
const ATTR_EXT_FEATURES: u16 = 217;
const EXT_FEATURE_SAE_OFFLOAD_AP: usize = 51;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Radio {
    pub bands: Vec<BandOffer>,
    /// WMM power save: a dozing phone collects its frames when it asks.
    pub ap_uapsd: bool,
    /// hostapd runs SAE itself, which needs a radio that leaves the access point's
    /// authentication to it.
    pub ap_sae: bool,
    /// The firmware runs SAE for the access point.
    pub sae_offload: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BandOffer {
    pub band: Band,
    pub ht: Option<u16>,
    pub vht: Option<u32>,
    /// The HE PHY capabilities an access point gets.
    pub he: Option<Vec<u8>>,
    pub eht: bool,
    /// The channel numbers an access point may start on.
    pub channels: Vec<u32>,
}

impl Radio {
    pub fn band(&self, band: Band) -> Option<&BandOffer> {
        self.bands.iter().find(|b| b.band == band)
    }

    fn band_mut(&mut self, band: Band) -> &mut BandOffer {
        match self.bands.iter().position(|b| b.band == band) {
            Some(at) => &mut self.bands[at],
            None => {
                self.bands.push(BandOffer {
                    band,
                    ht: None,
                    vht: None,
                    he: None,
                    eht: false,
                    channels: Vec::new(),
                });
                self.bands.last_mut().expect("just pushed")
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub fn radio(iface: &str) -> Option<Radio> {
    use crate::{
        ATTR_SPLIT_WIPHY_DUMP, ATTR_WIPHY, NL80211_CMD_GET_WIPHY, NLM_F_DUMP, attr, call,
        family_id, message, open,
    };
    let index: u32 = std::fs::read_to_string(format!("/sys/class/net/{iface}/phy80211/index"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let fd = open().ok()?;
    let family = family_id(&fd).ok()?;
    let mut wanted = attr(ATTR_SPLIT_WIPHY_DUMP, &[]);
    wanted.extend(attr(ATTR_WIPHY, &index.to_ne_bytes()));
    let payloads = call(&fd, &message(family, NL80211_CMD_GET_WIPHY, NLM_F_DUMP, &wanted)).ok()?;
    Some(radio_in(&payloads, index))
}

#[cfg(not(target_os = "linux"))]
pub fn radio(_iface: &str) -> Option<Radio> {
    None
}

/// A split dump spreads the radio over several messages.
fn radio_in(payloads: &[Vec<u8>], index: u32) -> Radio {
    use crate::{ATTR_WIPHY, ATTR_WIPHY_BANDS, Attrs};
    let mut radio = Radio::default();
    let mut device_sme = false;
    for payload in payloads {
        let ours = Attrs(&payload[..])
            .any(|(kind, value)| kind == ATTR_WIPHY && value.len() >= 4 && u32_of(value) == index);
        if !ours {
            continue;
        }
        for (kind, value) in Attrs(&payload[..]) {
            match kind {
                ATTR_WIPHY_BANDS => {
                    for (nl_band, attrs) in Attrs(value) {
                        if let Some(band) = band_of(nl_band) {
                            band_in(radio.band_mut(band), attrs);
                        }
                    }
                }
                ATTR_SUPPORT_AP_UAPSD => radio.ap_uapsd = true,
                ATTR_DEVICE_AP_SME => device_sme = true,
                ATTR_EXT_FEATURES => {
                    let bit = EXT_FEATURE_SAE_OFFLOAD_AP;
                    radio.sae_offload =
                        value.get(bit / 8).is_some_and(|b| b & (1 << (bit % 8)) != 0);
                }
                _ => {}
            }
        }
    }
    radio.ap_sae = !device_sme;
    radio.bands.retain(|b| b.ht.is_some() || b.he.is_some() || !b.channels.is_empty());
    radio.bands.sort_by_key(|b| b.band);
    radio
}

/// `enum nl80211_band`.
fn band_of(nl_band: u16) -> Option<Band> {
    match nl_band {
        0 => Some(Band::Ghz24),
        1 => Some(Band::Ghz5),
        3 => Some(Band::Ghz6),
        _ => None,
    }
}

fn band_in(offer: &mut BandOffer, attrs: &[u8]) {
    use crate::{Attrs, BAND_ATTR_FREQS, frequency};
    for (kind, value) in Attrs(attrs) {
        match kind {
            BAND_ATTR_FREQS => {
                for (_, f) in Attrs(value) {
                    let Some(f) = frequency(f).filter(|f| f.flags == "ok") else { continue };
                    if let Some(ch) = Channel::of_freq(f.freq).filter(|c| c.band == offer.band) {
                        offer.channels.push(ch.number);
                    }
                }
            }
            BAND_ATTR_HT_CAPA if value.len() >= 2 => {
                offer.ht = Some(u16::from_ne_bytes([value[0], value[1]]));
            }
            BAND_ATTR_VHT_CAPA if value.len() >= 4 => offer.vht = Some(u32_of(value)),
            BAND_ATTR_IFTYPE_DATA => {
                for (_, entry) in Attrs(value) {
                    iftype_in(offer, entry);
                }
            }
            _ => {}
        }
    }
    offer.channels.sort_unstable();
    offer.channels.dedup();
}

/// Only the access point's own capabilities count.
fn iftype_in(offer: &mut BandOffer, entry: &[u8]) {
    use crate::{Attrs, IFTYPE_AP};
    let mut ap = false;
    let mut he = None;
    let mut eht = false;
    for (kind, value) in Attrs(entry) {
        match kind {
            IFTYPE_ATTR_IFTYPES => ap = Attrs(value).any(|(t, _)| u32::from(t) == IFTYPE_AP),
            IFTYPE_ATTR_HE_CAP_PHY => he = Some(value.to_vec()),
            IFTYPE_ATTR_EHT_CAP_PHY => eht = true,
            _ => {}
        }
    }
    if ap {
        offer.he = he;
        offer.eht = eht;
    }
}

fn u32_of(v: &[u8]) -> u32 {
    u32::from_ne_bytes([v[0], v[1], v[2], v[3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ATTR_WIPHY, ATTR_WIPHY_BANDS, BAND_ATTR_FREQS, FREQ_ATTR_FREQ, FREQ_ATTR_NO_IR, IFTYPE_AP,
        attr,
    };

    fn freq(mhz: u32, no_ir: bool) -> Vec<u8> {
        let mut f = attr(FREQ_ATTR_FREQ, &mhz.to_ne_bytes());
        if no_ir {
            f.extend(attr(FREQ_ATTR_NO_IR, &[]));
        }
        f
    }

    fn iftype_entry(iftype: u32, he_phy: &[u8], eht: bool) -> Vec<u8> {
        let mut e = attr(IFTYPE_ATTR_IFTYPES, &attr(iftype as u16, &[]));
        e.extend(attr(IFTYPE_ATTR_HE_CAP_PHY, he_phy));
        if eht {
            e.extend(attr(IFTYPE_ATTR_EHT_CAP_PHY, &[0xe8, 0x0d]));
        }
        e
    }

    fn wiphy(index: u32, rest: &[u8]) -> Vec<u8> {
        let mut m = attr(ATTR_WIPHY, &index.to_ne_bytes());
        m.extend_from_slice(rest);
        m
    }

    fn mt7925_5ghz() -> Vec<u8> {
        let mut freqs = attr(0, &freq(5180, false));
        freqs.extend(attr(1, &freq(5260, true)));
        let mut band = attr(BAND_ATTR_FREQS, &freqs);
        band.extend(attr(BAND_ATTR_HT_CAPA, &0x09ffu16.to_ne_bytes()));
        band.extend(attr(BAND_ATTR_VHT_CAPA, &0x339071f6u32.to_ne_bytes()));
        let mut data = attr(0, &iftype_entry(2, &[0x4c, 0x70], false));
        data.extend(attr(1, &iftype_entry(IFTYPE_AP, &[0x0c, 0x20], true)));
        band.extend(attr(BAND_ATTR_IFTYPE_DATA, &data));
        attr(ATTR_WIPHY_BANDS, &attr(1, &band))
    }

    #[test]
    fn a_split_dump_adds_up_to_one_radio() {
        let six =
            attr(ATTR_WIPHY_BANDS, &attr(3, &attr(BAND_ATTR_FREQS, &attr(0, &freq(6135, false)))));
        let payloads = [
            wiphy(0, &mt7925_5ghz()),
            wiphy(0, &six),
            wiphy(0, &attr(ATTR_SUPPORT_AP_UAPSD, &[])),
            wiphy(1, &attr(ATTR_DEVICE_AP_SME, &0u32.to_ne_bytes())),
        ];
        let radio = radio_in(&payloads, 0);
        let five = radio.band(Band::Ghz5).unwrap();
        assert_eq!((five.ht, five.vht), (Some(0x09ff), Some(0x339071f6)));
        assert_eq!(five.he.as_deref(), Some(&[0x0c, 0x20][..]));
        assert!(five.eht);
        assert_eq!(five.channels, [36]);
        assert_eq!(radio.band(Band::Ghz6).unwrap().channels, [37]);
        assert!(radio.band(Band::Ghz24).is_none());
        assert!(radio.ap_uapsd);
        assert!(radio.ap_sae, "the other radio\x27s AP SME is not ours");
    }

    #[test]
    fn a_radio_that_runs_the_access_point_itself_needs_sae_offload() {
        let sme = wiphy(0, &attr(ATTR_DEVICE_AP_SME, &0u32.to_ne_bytes()));
        let radio = radio_in(&[wiphy(0, &mt7925_5ghz()), sme.clone()], 0);
        assert!(!radio.ap_sae);
        assert!(!radio.sae_offload);
        let mut features = vec![0u8; 8];
        features[EXT_FEATURE_SAE_OFFLOAD_AP / 8] |= 1 << (EXT_FEATURE_SAE_OFFLOAD_AP % 8);
        let offload = wiphy(0, &attr(ATTR_EXT_FEATURES, &features));
        assert!(radio_in(&[sme, offload], 0).sae_offload);
    }

    #[test]
    fn a_client_only_he_radio_offers_an_access_point_no_he() {
        let mut band = attr(BAND_ATTR_HT_CAPA, &0x0062u16.to_ne_bytes());
        band.extend(attr(BAND_ATTR_IFTYPE_DATA, &attr(0, &iftype_entry(2, &[0x4c], true))));
        let radio = radio_in(&[wiphy(0, &attr(ATTR_WIPHY_BANDS, &attr(0, &band)))], 0);
        let two = radio.band(Band::Ghz24).unwrap();
        assert_eq!(two.ht, Some(0x0062));
        assert!(two.he.is_none());
        assert!(!two.eht);
    }
}
