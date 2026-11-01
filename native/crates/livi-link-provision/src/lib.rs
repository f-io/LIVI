// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

pub mod detect;
pub mod dongle;

pub fn tilde(path: &Path) -> String {
    shorten(
        &path.display().to_string(),
        std::env::var("HOME").ok().as_deref(),
    )
}

fn shorten(text: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|h| !h.is_empty()) else {
        return text.into();
    };
    match text.strip_prefix(home) {
        Some("") => "~".into(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::shorten;

    #[test]
    fn the_home_becomes_a_tilde() {
        let home = Some("/Users/x");
        assert_eq!(shorten("/Users/x/Library/LIVI", home), "~/Library/LIVI");
        assert_eq!(shorten("/Users/x", home), "~");
        assert_eq!(shorten("/Users/xy/LIVI", home), "/Users/xy/LIVI");
        assert_eq!(shorten("/tmp/LIVI", home), "/tmp/LIVI");
        assert_eq!(shorten("/Users/x/LIVI", None), "/Users/x/LIVI");
    }
}
