// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::{Map, Value};

pub fn get<'a>(config: &'a Value, path: &str) -> &'a Value {
    path.split('.')
        .filter(|p| !p.is_empty())
        .fold(config, |v, key| &v[key])
}

pub fn patch(path: &str, value: Value) -> Value {
    path.rsplit('.')
        .filter(|p| !p.is_empty())
        .fold(value, |inner, key| {
            let mut m = Map::new();
            m.insert(key.to_string(), inner);
            Value::Object(m)
        })
}

pub fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

pub fn same(a: &Value, b: &Value) -> bool {
    match (as_f64(a), as_f64(b)) {
        (Some(x), Some(y)) if a.is_number() || b.is_number() => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn nested_paths_read_and_patch() {
        let cfg = json!({ "displayMode": { "main": "1920x1080" }, "kiosk": true });
        assert_eq!(get(&cfg, "displayMode.main"), &json!("1920x1080"));
        assert_eq!(get(&cfg, "kiosk"), &json!(true));
        assert!(get(&cfg, "missing.deep").is_null());
        assert_eq!(
            patch("displayMode.main", json!("auto")),
            json!({ "displayMode": { "main": "auto" } })
        );
    }

    #[test]
    fn numbers_match_across_types() {
        assert!(same(&json!(1), &json!(1.0)));
        assert!(same(&json!("2"), &json!(2)));
        assert!(!same(&json!("a"), &json!("b")));
    }
}
