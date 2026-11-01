// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub const PATH: &str = "/tmp/livi/radio.conf";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Radio {
    Wifi,
    Bt,
}

impl Radio {
    fn key(self) -> &'static str {
        match self {
            Radio::Wifi => "wifi",
            Radio::Bt => "bt",
        }
    }
}

pub fn enabled(radio: Radio) -> bool {
    enabled_in(&std::fs::read_to_string(PATH).unwrap_or_default(), radio)
}

pub fn set(radio: Radio, on: bool) -> std::io::Result<bool> {
    let text = std::fs::read_to_string(PATH).unwrap_or_default();
    if enabled_in(&text, radio) == on {
        return Ok(false);
    }
    if let Some(dir) = std::path::Path::new(PATH).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = format!("{PATH}.new");
    std::fs::write(&temp, with(&text, radio, on))?;
    std::fs::rename(&temp, PATH)?;
    Ok(true)
}

fn enabled_in(text: &str, radio: Radio) -> bool {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .rfind(|(key, _)| key.trim() == radio.key())
        .is_none_or(|(_, value)| value.trim() != "off")
}

fn with(text: &str, radio: Radio, on: bool) -> String {
    let mut out: String = text
        .lines()
        .filter(|line| {
            line.split_once('=')
                .is_none_or(|(key, _)| key.trim() != radio.key())
        })
        .map(|line| format!("{line}\n"))
        .collect();
    out.push_str(&format!(
        "{}={}\n",
        radio.key(),
        if on { "on" } else { "off" }
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_stored_is_on() {
        assert!(enabled_in("", Radio::Wifi));
        assert!(enabled_in("", Radio::Bt));
    }

    #[test]
    fn each_radio_switches_on_its_own() {
        let text = with("", Radio::Wifi, false);
        assert!(!enabled_in(&text, Radio::Wifi));
        assert!(enabled_in(&text, Radio::Bt));

        let text = with(&text, Radio::Bt, false);
        assert!(!enabled_in(&text, Radio::Wifi));
        assert!(!enabled_in(&text, Radio::Bt));

        let text = with(&text, Radio::Wifi, true);
        assert!(enabled_in(&text, Radio::Wifi));
        assert!(!enabled_in(&text, Radio::Bt));
        assert_eq!(text.matches("wifi=").count(), 1);
    }

    #[test]
    fn other_lines_are_kept() {
        let text = with("# kept\nled=on\n", Radio::Bt, false);
        assert!(text.contains("# kept\n"));
        assert!(text.contains("led=on\n"));
        assert!(!enabled_in(&text, Radio::Bt));
    }
}
