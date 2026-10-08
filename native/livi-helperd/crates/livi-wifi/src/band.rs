//! Channel numbers repeat across the bands, so a channel always carries its band.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Band {
    Ghz24,
    Ghz5,
    Ghz6,
}

impl Band {
    /// The name the settings keep.
    pub fn setting(self) -> &'static str {
        match self {
            Band::Ghz24 => "2.4ghz",
            Band::Ghz5 => "5ghz",
            Band::Ghz6 => "6ghz",
        }
    }

    pub fn of_setting(name: &str) -> Option<Self> {
        [Band::Ghz24, Band::Ghz5, Band::Ghz6].into_iter().find(|b| b.setting() == name)
    }

    /// hostapd's `hw_mode`.
    pub fn hw_mode(self) -> &'static str {
        match self {
            Band::Ghz24 => "g",
            Band::Ghz5 | Band::Ghz6 => "a",
        }
    }
}

impl fmt::Display for Band {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Band::Ghz24 => "2.4 GHz",
            Band::Ghz5 => "5 GHz",
            Band::Ghz6 => "6 GHz",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Channel {
    pub band: Band,
    pub number: u32,
}

impl Channel {
    pub fn new(band: Band, number: u32) -> Self {
        Self { band, number }
    }

    /// A channel kept as a bare number, as settings and the dongle did before 6 GHz.
    pub fn of_number(number: u32) -> Self {
        let band = if number <= 14 { Band::Ghz24 } else { Band::Ghz5 };
        Self { band, number }
    }

    /// A channel as the settings keep it, a number and the band picked. Below 6 GHz the number
    /// alone decides, as it always did.
    pub fn of_setting(band: Band, number: u32) -> Self {
        if band == Band::Ghz6 { Self::new(band, number) } else { Self::of_number(number) }
    }

    pub fn of_freq(mhz: u32) -> Option<Self> {
        let (band, number) = match mhz {
            2484 => (Band::Ghz24, 14),
            2412..=2472 => (Band::Ghz24, (mhz - 2407) / 5),
            5000..=5895 => (Band::Ghz5, (mhz - 5000) / 5),
            5935 => (Band::Ghz6, 2),
            5955..=7115 => (Band::Ghz6, (mhz - 5950) / 5),
            _ => return None,
        };
        Some(Self { band, number })
    }

    pub fn freq_mhz(self) -> u32 {
        match (self.band, self.number) {
            (Band::Ghz24, 14) => 2484,
            (Band::Ghz24, n) => 2407 + 5 * n,
            (Band::Ghz5, n) => 5000 + 5 * n,
            (Band::Ghz6, 2) => 5935,
            (Band::Ghz6, n) => 5950 + 5 * n,
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} MHz)", self.number, self.freq_mhz())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_band_maps_its_channels_to_frequencies_and_back() {
        for (band, number, mhz) in [
            (Band::Ghz24, 1, 2412),
            (Band::Ghz24, 13, 2472),
            (Band::Ghz24, 14, 2484),
            (Band::Ghz5, 36, 5180),
            (Band::Ghz5, 165, 5825),
            (Band::Ghz6, 2, 5935),
            (Band::Ghz6, 1, 5955),
            (Band::Ghz6, 37, 6135),
            (Band::Ghz6, 233, 7115),
        ] {
            let channel = Channel::new(band, number);
            assert_eq!(channel.freq_mhz(), mhz, "{band} {number}");
            assert_eq!(Channel::of_freq(mhz), Some(channel), "{mhz}");
        }
        assert_eq!(Channel::of_freq(1000), None);
    }

    #[test]
    fn a_bare_number_names_a_channel_below_6_ghz() {
        assert_eq!(Channel::of_number(6), Channel::new(Band::Ghz24, 6));
        assert_eq!(Channel::of_number(36), Channel::new(Band::Ghz5, 36));
        assert_eq!(Channel::of_number(149).freq_mhz(), 5745);
    }

    #[test]
    fn the_settings_name_every_band_and_a_6_ghz_number_keeps_its_band() {
        for band in [Band::Ghz24, Band::Ghz5, Band::Ghz6] {
            assert_eq!(Band::of_setting(band.setting()), Some(band));
        }
        assert_eq!(Band::of_setting("60ghz"), None);
        assert_eq!(Channel::of_setting(Band::Ghz6, 37), Channel::new(Band::Ghz6, 37));
        assert_eq!(Channel::of_setting(Band::Ghz5, 6), Channel::new(Band::Ghz24, 6));
    }

    #[test]
    fn hostapd_runs_everything_above_2_4_ghz_as_mode_a() {
        assert_eq!(Band::Ghz24.hw_mode(), "g");
        assert_eq!(Band::Ghz5.hw_mode(), "a");
        assert_eq!(Band::Ghz6.hw_mode(), "a");
    }
}
