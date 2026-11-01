// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Main,
    Cluster,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

fn prefix(stream: Stream) -> &'static str {
    match stream {
        Stream::Main => "projection",
        Stream::Cluster => "cluster",
    }
}

fn number(config: &Value, key: String) -> u32 {
    config[key.as_str()]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
}

pub fn mask(state: &Value, stream: Stream) -> Option<Mask> {
    if state["sessions"]["active"].as_str() != Some("androidauto") {
        return None;
    }
    let config = &state["config"];
    let p = prefix(stream);
    let m = Mask {
        width: number(config, format!("{p}Width")),
        height: number(config, format!("{p}Height")),
        top: number(config, format!("{p}ViewAreaTop")),
        bottom: number(config, format!("{p}ViewAreaBottom")),
        left: number(config, format!("{p}ViewAreaLeft")),
        right: number(config, format!("{p}ViewAreaRight")),
    };
    let insets = m.top + m.bottom + m.left + m.right;
    (m.width > 0 && m.height > 0 && insets > 0).then_some(m)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn state(active: Value) -> Value {
        json!({
            "sessions": { "active": active },
            "config": {
                "projectionWidth": 1280,
                "projectionHeight": 720,
                "projectionViewAreaTop": 20,
                "projectionViewAreaBottom": 0,
                "projectionViewAreaLeft": 100,
                "projectionViewAreaRight": 0,
                "clusterWidth": 800,
                "clusterHeight": 480,
                "clusterViewAreaTop": 0,
                "clusterViewAreaBottom": 0,
                "clusterViewAreaLeft": 0,
                "clusterViewAreaRight": 0
            }
        })
    }

    #[test]
    fn only_android_auto_gets_a_mask() {
        assert_eq!(mask(&state(json!("carplay")), Stream::Main), None);
        assert_eq!(mask(&state(Value::Null), Stream::Main), None);
        assert_eq!(
            mask(&state(json!("androidauto")), Stream::Main),
            Some(Mask {
                width: 1280,
                height: 720,
                top: 20,
                bottom: 0,
                left: 100,
                right: 0,
            })
        );
    }

    #[test]
    fn each_stream_uses_its_own_values() {
        let mut s = state(json!("androidauto"));
        assert_eq!(mask(&s, Stream::Cluster), None);
        s["config"]["clusterViewAreaBottom"] = json!(30);
        assert_eq!(
            mask(&s, Stream::Cluster),
            Some(Mask {
                width: 800,
                height: 480,
                bottom: 30,
                ..Mask::default()
            })
        );
    }
}
