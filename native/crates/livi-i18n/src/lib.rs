// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;

use serde_json::Value;

const EN: &str = include_str!("../locales/en.json");

pub const LOCALES: [(&str, Option<[u8; 4]>, &str); 32] = [
    ("cs", None, include_str!("../locales/cs.json")),
    ("da", None, include_str!("../locales/da.json")),
    ("de", None, include_str!("../locales/de.json")),
    ("et", None, include_str!("../locales/et.json")),
    ("en", None, EN),
    ("es", None, include_str!("../locales/es.json")),
    ("fr", None, include_str!("../locales/fr.json")),
    ("ga", None, include_str!("../locales/ga.json")),
    ("hr", None, include_str!("../locales/hr.json")),
    ("it", None, include_str!("../locales/it.json")),
    ("lv", None, include_str!("../locales/lv.json")),
    ("lt", None, include_str!("../locales/lt.json")),
    ("hu", None, include_str!("../locales/hu.json")),
    ("mt", None, include_str!("../locales/mt.json")),
    ("nl", None, include_str!("../locales/nl.json")),
    ("pl", None, include_str!("../locales/pl.json")),
    ("pt", None, include_str!("../locales/pt.json")),
    ("ro", None, include_str!("../locales/ro.json")),
    ("sk", None, include_str!("../locales/sk.json")),
    ("sl", None, include_str!("../locales/sl.json")),
    ("fi", None, include_str!("../locales/fi.json")),
    ("sv", None, include_str!("../locales/sv.json")),
    ("el", None, include_str!("../locales/el.json")),
    ("bg", None, include_str!("../locales/bg.json")),
    ("uk", None, include_str!("../locales/uk.json")),
    ("he", Some(*b"Hebr"), include_str!("../locales/he.json")),
    ("ar", Some(*b"Arab"), include_str!("../locales/ar.json")),
    ("hi", Some(*b"Deva"), include_str!("../locales/hi.json")),
    ("th", Some(*b"Thai"), include_str!("../locales/th.json")),
    ("ja", Some(*b"Hira"), include_str!("../locales/ja.json")),
    ("zh", Some(*b"Hani"), include_str!("../locales/zh.json")),
    ("ko", Some(*b"Hang"), include_str!("../locales/ko.json")),
];

pub struct Strings {
    lang: String,
    words: HashMap<String, String>,
    fallback: HashMap<String, String>,
    scripts: Option<Vec<[u8; 4]>>,
}

fn flatten(prefix: &str, value: &Value, out: &mut HashMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

fn table(json: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(v) = serde_json::from_str::<Value>(json) {
        flatten("", &v, &mut out);
    }
    out
}

fn locale(lang: &str) -> Option<&'static (&'static str, Option<[u8; 4]>, &'static str)> {
    LOCALES.iter().find(|(c, _, _)| *c == lang)
}

fn source(lang: &str) -> &'static str {
    locale(lang).map_or(EN, |(_, _, json)| json)
}

pub fn translated(lang: &str) -> bool {
    !table(source(lang)).is_empty()
}

pub fn script(lang: &str) -> Option<[u8; 4]> {
    locale(lang).and_then(|(_, script, _)| *script)
}

impl Strings {
    pub fn new(lang: &str) -> Self {
        Self {
            lang: lang.to_string(),
            words: table(source(lang)),
            fallback: table(EN),
            scripts: None,
        }
    }

    pub fn readable(lang: &str, scripts: &[[u8; 4]]) -> Self {
        let shown = script(lang).is_none_or(|s| scripts.contains(&s));
        Self {
            lang: lang.to_string(),
            words: table(if shown { source(lang) } else { EN }),
            fallback: table(EN),
            scripts: Some(scripts.to_vec()),
        }
    }

    pub fn lang(&self) -> &str {
        &self.lang
    }

    pub fn scripts(&self) -> Option<&[[u8; 4]]> {
        self.scripts.as_deref()
    }

    pub fn shows(&self, script: [u8; 4]) -> bool {
        self.scripts.as_ref().is_none_or(|s| s.contains(&script))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.words
            .get(key)
            .or_else(|| self.fallback.get(key))
            .map(String::as_str)
    }

    pub fn t(&self, key: &str, text: &str) -> String {
        self.get(key).unwrap_or(text).to_string()
    }

    pub fn with(&self, key: &str, text: &str, vars: &[(&str, &str)]) -> String {
        let mut s = self.t(key, text);
        for (name, value) in vars {
            s = s.replace(&format!("{{{{{name}}}}}"), value);
        }
        s
    }
}

pub fn cached(lang: &str) -> &'static Strings {
    static CACHE: std::sync::Mutex<Vec<&'static Strings>> = std::sync::Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(strings) = cache.iter().find(|s| s.lang == lang) {
        return strings;
    }
    let strings: &'static Strings = Box::leak(Box::new(Strings::new(lang)));
    cache.push(strings);
    strings
}

impl Default for Strings {
    fn default() -> Self {
        Self::new("en")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_keys_resolve_with_a_fallback() {
        let en = Strings::new("en");
        assert_eq!(en.t("settings.audio", "x"), "Audio");
        assert_eq!(en.t("no.such.key", "Fallback"), "Fallback");
    }

    #[test]
    fn a_language_without_its_script_reads_english() {
        let no_font = Strings::readable("ja", &[*b"Hani"]);
        assert_eq!(no_font.lang(), "ja");
        assert!(!no_font.shows(*b"Hira"));
        assert_eq!(
            no_font.t("settings.audio", "x"),
            Strings::new("en").t("settings.audio", "x")
        );
        assert!(Strings::readable("ja", &[*b"Hira"]).shows(*b"Hira"));
        assert!(Strings::new("de").shows(*b"Arab"));
        assert_eq!(script("de"), None);
        assert_eq!(script("he"), Some(*b"Hebr"));
    }

    fn vars(text: &str) -> Vec<&str> {
        let mut out: Vec<&str> = text
            .split("{{")
            .skip(1)
            .filter_map(|rest| rest.split_once("}}").map(|(name, _)| name))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn every_locale_is_empty_or_complete() {
        let en = table(EN);
        for (name, _, json) in LOCALES {
            assert!(
                serde_json::from_str::<Value>(json).is_ok(),
                "{name}.json is not valid JSON"
            );
            let words = table(json);
            if words.is_empty() {
                continue;
            }
            let missing: Vec<_> = en.keys().filter(|k| !words.contains_key(*k)).collect();
            let unknown: Vec<_> = words.keys().filter(|k| !en.contains_key(*k)).collect();
            let empty: Vec<_> = words
                .iter()
                .filter(|(_, v)| v.trim().is_empty())
                .map(|(k, _)| k)
                .collect();
            let placeholders: Vec<_> = en
                .iter()
                .filter(|(k, v)| words.get(*k).is_some_and(|w| vars(w) != vars(v)))
                .map(|(k, _)| k)
                .collect();
            assert!(
                missing.is_empty()
                    && unknown.is_empty()
                    && empty.is_empty()
                    && placeholders.is_empty(),
                "{name}.json: missing {missing:?}, unknown {unknown:?}, empty {empty:?}, placeholders differ in {placeholders:?}"
            );
        }
    }

    #[test]
    fn only_filled_locales_count_as_translated() {
        for (name, _, json) in LOCALES {
            assert_eq!(translated(name), !table(json).is_empty(), "{name}");
        }
    }

    #[test]
    fn placeholders_are_filled() {
        let en = Strings::new("en");
        assert_eq!(en.with("none", "Hi {{name}}", &[("name", "Ada")]), "Hi Ada");
    }
}
