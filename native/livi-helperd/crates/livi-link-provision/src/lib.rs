//! Provisions dongles. What a whole architecture family shares lives in `dongle`, what belongs to
//! one SoC sits below it in `dongle::arm::<soc>` or `dongle::riscv::<soc>`.

use std::path::Path;

pub mod detect;
pub mod dongle;

pub fn tilde(path: &Path) -> String {
    shorten(&path.display().to_string(), std::env::var("HOME").ok().as_deref())
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
        // A longer name that merely starts the same is not the home.
        assert_eq!(shorten("/Users/xy/LIVI", home), "/Users/xy/LIVI");
        assert_eq!(shorten("/tmp/LIVI", home), "/tmp/LIVI");
        assert_eq!(shorten("/Users/x/LIVI", None), "/Users/x/LIVI");
    }
}
