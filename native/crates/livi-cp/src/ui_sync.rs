// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::bplist;
use crate::opack;

pub const FEATURE: &str = "uiSync";
pub const INFO_KEY: &str = "uiSyncInfo";
pub const DATASTREAM_UUID: &str = "07D9F906-8D64-4B54-A808-A20BCA2C51C2";

const RESET: u64 = 0;
const RESET_COMPLETE: u64 = 1;
const COMMAND: u64 = 2;
const COMMAND_ACK: u64 = 3;
const VERSION: u64 = 2;

const START_LAYOUT_TRANSITION: u64 = 0x03;
const REQUEST_LAYOUT: u64 = 0x05;
const PALETTE_CHANGE: u64 = 0x10;
const GIVE_FOCUS: u64 = 0x07;
const REQUEST_FOCUS: u64 = 0x08;
const ALLOW_LAYOUT_CHANGE: u64 = 0x11;
const FOCUS_TOKEN: u64 = 4711;
const APPEARANCE_PREFERENCE_CHANGE: u64 = 0x13;
const LIGHT: u64 = 0;
const DARK: u64 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum PhoneChoice {
    Layout(String),
    Palette { layout: String, palette: String },
    Appearance(Appearance),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Automatic,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub id: String,
    pub palette: String,
    pub active: Vec<(String, String)>,
}

#[derive(Default)]
pub struct UiSync {
    session: u64,
    next_tx: u64,
    last_rx: u64,
    ready: bool,
    awaiting_reset_complete: bool,
    phone_id: Option<bplist::Value>,
    version: u64,
    panel: Vec<(opack::Value, opack::Value)>,
    wanted: Option<Layout>,
    light: bool,
    choices: Vec<PhoneChoice>,
}

impl UiSync {
    pub fn reopen(&mut self) {
        *self = Self {
            wanted: self.wanted.take(),
            light: self.light,
            ..Self::default()
        };
    }

    pub fn select(&mut self, layout: Layout) -> Vec<Vec<u8>> {
        self.wanted = Some(layout);
        let Some(request) = self.request().filter(|_| self.ready) else {
            return Vec::new();
        };
        let out = vec![self.message(COMMAND, Some(request), Vec::new())];
        println!("[cpUiSync] sent: {}", describe(&out[0]));
        out
    }

    pub fn set_dark(&mut self, dark: bool) -> Vec<Vec<u8>> {
        if self.light != dark {
            return Vec::new();
        }
        self.light = !dark;
        match self.wanted.take() {
            Some(layout) => self.select(layout),
            None => Vec::new(),
        }
    }

    pub fn take_choices(&mut self) -> Vec<PhoneChoice> {
        std::mem::take(&mut self.choices)
    }

    fn request(&self) -> Option<bplist::Value> {
        self.wanted
            .as_ref()
            .map(|layout| request_layout(layout, if self.light { LIGHT } else { DARK }))
    }

    pub fn handle(&mut self, message: &[u8]) -> Vec<Vec<u8>> {
        println!("[cpUiSync] {}", describe(message));
        let Some(opack::Value::Dict(frame)) = opack::decode(message) else {
            return Vec::new();
        };
        let data = opack::Value::str("data");
        let Some(header) = frame
            .iter()
            .find(|(k, _)| *k == data)
            .and_then(|(_, v)| v.as_data())
        else {
            return Vec::new();
        };
        let Ok(header) = bplist::decode(header) else {
            return Vec::new();
        };
        self.panel = frame.into_iter().filter(|(k, _)| *k != data).collect();
        let int = |k| header.get(k).and_then(bplist::Value::as_int);
        let mut out = Vec::new();
        match int("type") {
            Some(RESET) => {
                if self.awaiting_reset_complete && self.next_tx == 2 {
                    return out;
                }
                self.session += 1;
                self.next_tx = 1;
                self.last_rx = 0;
                self.ready = false;
                self.awaiting_reset_complete = true;
                self.phone_id = header.get("id").cloned();
                let version = vec![("version", bplist::Value::Int(VERSION))];
                out.push(self.message(RESET, None, self.with_id(version)));
            }
            _ if int("ssn") != Some(self.session) => {}
            Some(RESET_COMPLETE) if self.awaiting_reset_complete => {
                self.awaiting_reset_complete = false;
                self.ready = true;
                self.version = int("version").unwrap_or(1).min(VERSION);
                self.last_rx = int("psn").unwrap_or(0);
                let mut fields = vec![("version", bplist::Value::Int(self.version))];
                if self.version >= 2 {
                    fields = self.with_id(fields);
                }
                out.push(self.message(RESET_COMPLETE, None, fields));
                out.push(self.message(COMMAND, Some(allow_layout_change()), Vec::new()));
                if let Some(request) = self.request() {
                    out.push(self.message(COMMAND, Some(request), Vec::new()));
                }
                out.push(self.message(COMMAND, Some(give_focus()), Vec::new()));
            }
            Some(COMMAND) if self.ready => {
                self.last_rx = int("psn").unwrap_or(self.last_rx);
                let wants_focus = self.follow_phone(&header);
                if header.get("noAck").and_then(bplist::Value::as_bool) != Some(true) {
                    out.push(self.message(COMMAND_ACK, None, Vec::new()));
                }
                if wants_focus {
                    out.push(self.message(COMMAND, Some(give_focus()), Vec::new()));
                }
            }
            Some(COMMAND_ACK) => self.last_rx = int("psn").unwrap_or(self.last_rx),
            _ => {}
        }
        for m in &out {
            println!("[cpUiSync] sent: {}", describe(m));
        }
        out
    }

    fn follow_phone(&mut self, header: &bplist::Value) -> bool {
        let Some(Ok(command)) = header
            .get("data")
            .and_then(bplist::Value::as_data)
            .map(bplist::decode)
        else {
            return false;
        };
        let data = command.get("data");
        let text = |k: &str| data.and_then(|d| d.get(k)).and_then(bplist::Value::as_str);
        match command.get("type").and_then(bplist::Value::as_int) {
            Some(START_LAYOUT_TRANSITION) => {
                let Some(id) = text("layoutID") else {
                    return false;
                };
                self.choices.push(PhoneChoice::Layout(id.to_string()));
                if let Some(palette) = text("paletteID") {
                    let (layout, palette) = (id.to_string(), palette.to_string());
                    self.choices.push(PhoneChoice::Palette { layout, palette });
                }
            }
            Some(PALETTE_CHANGE) => {
                let (Some(layout), Some(palette)) =
                    (text("layoutIdentifier"), text("paletteIdentifier"))
                else {
                    return false;
                };
                let (layout, palette) = (layout.to_string(), palette.to_string());
                self.choices.push(PhoneChoice::Palette { layout, palette });
            }
            Some(APPEARANCE_PREFERENCE_CHANGE) => {
                let preference = data.and_then(|d| d.get("preference"));
                let appearance = match preference.and_then(bplist::Value::as_int) {
                    Some(0) => Appearance::Automatic,
                    Some(1) => Appearance::Light,
                    Some(2) => Appearance::Dark,
                    _ => return false,
                };
                self.choices.push(PhoneChoice::Appearance(appearance));
            }
            Some(REQUEST_FOCUS) => return true,
            _ => {}
        }
        false
    }

    fn with_id<'a>(
        &self,
        mut fields: Vec<(&'a str, bplist::Value)>,
    ) -> Vec<(&'a str, bplist::Value)> {
        if let Some(id) = &self.phone_id {
            fields.push(("id", id.clone()));
        }
        fields
    }

    fn message(
        &mut self,
        kind: u64,
        data: Option<bplist::Value>,
        extra: Vec<(&str, bplist::Value)>,
    ) -> Vec<u8> {
        let mut header = vec![
            ("type".to_string(), bplist::Value::Int(kind)),
            ("ssn".to_string(), bplist::Value::Int(self.session)),
            ("psn".to_string(), bplist::Value::Int(self.next_tx)),
            ("asn".to_string(), bplist::Value::Int(self.last_rx)),
        ];
        header.extend(extra.into_iter().map(|(k, v)| (k.to_string(), v)));
        if let Some(data) = data {
            header.push((
                "data".to_string(),
                bplist::Value::Data(bplist::encode(&data)),
            ));
        }
        self.next_tx += 1;
        let mut frame = self.panel.clone();
        let header = bplist::encode(&bplist::Value::Dict(header));
        frame.push((opack::Value::str("data"), opack::Value::Data(header)));
        opack::encode(&opack::Value::Dict(frame))
    }
}

fn give_focus() -> bplist::Value {
    bplist::Value::Dict(vec![
        ("type".into(), bplist::Value::Int(GIVE_FOCUS)),
        (
            "data".into(),
            bplist::Value::Dict(vec![("tokenID".into(), bplist::Value::Int(FOCUS_TOKEN))]),
        ),
    ])
}

fn allow_layout_change() -> bplist::Value {
    bplist::Value::Dict(vec![
        ("type".into(), bplist::Value::Int(ALLOW_LAYOUT_CHANGE)),
        (
            "data".into(),
            bplist::Value::Dict(vec![("allow".into(), bplist::Value::Bool(true))]),
        ),
    ])
}

fn request_layout(layout: &Layout, appearance: u64) -> bplist::Value {
    let s = |v: &str| bplist::Value::String(v.into());
    bplist::Value::Dict(vec![
        ("type".into(), bplist::Value::Int(REQUEST_LAYOUT)),
        (
            "data".into(),
            bplist::Value::Dict(vec![
                ("layoutID".into(), s(&layout.id)),
                ("paletteID".into(), s(&layout.palette)),
                ("appearanceMode".into(), bplist::Value::Int(appearance)),
                ("fadeInNewLayout".into(), bplist::Value::Bool(true)),
                ("fadeOutOldLayout".into(), bplist::Value::Bool(true)),
                (
                    "activeComponents".into(),
                    bplist::Value::Dict(
                        layout
                            .active
                            .iter()
                            .map(|(zone, c)| (zone.clone(), s(c)))
                            .collect(),
                    ),
                ),
            ]),
        ),
    ])
}

fn describe(message: &[u8]) -> String {
    let n = message.len();
    let Some(frame) = opack::decode(message) else {
        return format!("{n} B {}", hex(message));
    };
    let panel = frame
        .get("displayUUID")
        .and_then(opack::Value::as_str)
        .unwrap_or("?");
    let Some(data) = frame.get("data").and_then(opack::Value::as_data) else {
        return format!("{n} B frame without data: {frame:?}");
    };
    let Ok(header) = bplist::decode(data) else {
        return format!("{n} B for {panel}, data {}", hex(data));
    };
    match header
        .get("data")
        .and_then(bplist::Value::as_data)
        .map(bplist::decode)
    {
        Some(Ok(command)) => format!("{n} B for {panel}: {header:?} command {command:?}"),
        _ => format!("{n} B for {panel}: {header:?}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    let shown: String = bytes.iter().take(512).map(|b| format!("{b:02x}")).collect();
    format!("hex {shown}{}", if bytes.len() > 512 { "…" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bplist::Value::{Int, String as S};

    fn framed(header: Vec<(&str, bplist::Value)>) -> Vec<u8> {
        let header = header
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let data = bplist::encode(&bplist::Value::Dict(header));
        opack::encode(&opack::Value::Dict(vec![
            (
                opack::Value::str("displayUUID"),
                opack::Value::str("Cluster_Display"),
            ),
            (opack::Value::str("data"), opack::Value::Data(data)),
        ]))
    }

    fn header_of(out: &[u8]) -> bplist::Value {
        let frame = opack::decode(out).unwrap();
        assert_eq!(
            frame.get("displayUUID").and_then(opack::Value::as_str),
            Some("Cluster_Display")
        );
        bplist::decode(frame.get("data").and_then(opack::Value::as_data).unwrap()).unwrap()
    }

    fn int(h: &bplist::Value, k: &str) -> Option<u64> {
        h.get(k).and_then(bplist::Value::as_int)
    }

    #[test]
    fn the_car_answers_a_reset_and_asks_for_its_layout() {
        let layout = Layout {
            id: "0D38B526".into(),
            palette: "blue".into(),
            active: vec![("F0786093".into(), "2B28C705".into())],
        };
        let mut sync = UiSync::default();
        assert!(
            sync.select(layout).is_empty(),
            "nothing before the channel is up"
        );
        let id = S("89858A51".into());
        let reset = framed(vec![
            ("type", Int(0)),
            ("ssn", Int(1)),
            ("psn", Int(1)),
            ("asn", Int(0)),
            ("id", id.clone()),
            ("version", Int(3)),
        ]);
        let out = sync.handle(&reset);
        let h = header_of(&out[0]);
        assert_eq!(
            (int(&h, "type"), int(&h, "ssn"), int(&h, "psn")),
            (Some(0), Some(1), Some(1))
        );
        assert_eq!((int(&h, "version"), h.get("id")), (Some(2), Some(&id)));

        let complete = framed(vec![
            ("type", Int(1)),
            ("ssn", Int(1)),
            ("psn", Int(1)),
            ("version", Int(3)),
        ]);
        let out = sync.handle(&complete);
        assert_eq!(out.len(), 4);
        let h = header_of(&out[0]);
        assert_eq!(
            (int(&h, "type"), int(&h, "psn"), int(&h, "asn")),
            (Some(1), Some(2), Some(1))
        );
        let h = header_of(&out[1]);
        let command =
            bplist::decode(h.get("data").and_then(bplist::Value::as_data).unwrap()).unwrap();
        assert_eq!(int(&command, "type"), Some(ALLOW_LAYOUT_CHANGE));
        let allow = command.get("data").and_then(|d| d.get("allow"));
        assert_eq!(allow.and_then(bplist::Value::as_bool), Some(true));
        let h = header_of(&out[2]);
        assert_eq!((int(&h, "type"), int(&h, "psn")), (Some(2), Some(4)));
        let command =
            bplist::decode(h.get("data").and_then(bplist::Value::as_data).unwrap()).unwrap();
        assert_eq!(int(&command, "type"), Some(REQUEST_LAYOUT));
        let data = command.get("data").unwrap();
        assert_eq!(
            data.get("layoutID").and_then(bplist::Value::as_str),
            Some("0D38B526")
        );
        let active = data.get("activeComponents").unwrap();
        assert_eq!(
            active.get("F0786093").and_then(bplist::Value::as_str),
            Some("2B28C705")
        );
        let h = header_of(&out[3]);
        let command =
            bplist::decode(h.get("data").and_then(bplist::Value::as_data).unwrap()).unwrap();
        assert_eq!(
            int(&command, "type"),
            Some(GIVE_FOCUS),
            "LIVI has nothing to focus itself"
        );
        let token = command.get("data").and_then(|d| d.get("tokenID"));
        assert_eq!(token.and_then(bplist::Value::as_int), Some(FOCUS_TOKEN));

        let from_phone = framed(vec![("type", Int(2)), ("ssn", Int(1)), ("psn", Int(2))]);
        let h = header_of(&sync.handle(&from_phone)[0]);
        assert_eq!((int(&h, "type"), int(&h, "asn")), (Some(3), Some(2)));

        let other = Layout {
            id: "DB7C6944".into(),
            palette: "dark".into(),
            active: vec![],
        };
        let h = header_of(&sync.select(other)[0]);
        let command =
            bplist::decode(h.get("data").and_then(bplist::Value::as_data).unwrap()).unwrap();
        let data = command.get("data").unwrap();
        assert_eq!(
            data.get("layoutID").and_then(bplist::Value::as_str),
            Some("DB7C6944")
        );
        assert_eq!(
            data.get("paletteID").and_then(bplist::Value::as_str),
            Some("dark")
        );
        assert_eq!(
            data.get("appearanceMode").and_then(bplist::Value::as_int),
            Some(DARK)
        );

        assert!(sync.set_dark(true).is_empty(), "dark already");
        let h = header_of(&sync.set_dark(false)[0]);
        let command =
            bplist::decode(h.get("data").and_then(bplist::Value::as_data).unwrap()).unwrap();
        let data = command.get("data").unwrap();
        assert_eq!(
            data.get("layoutID").and_then(bplist::Value::as_str),
            Some("DB7C6944")
        );
        assert_eq!(
            data.get("appearanceMode").and_then(bplist::Value::as_int),
            Some(LIGHT)
        );
        assert!(
            UiSync::default().set_dark(false).is_empty(),
            "no layout asked for yet"
        );

        let from_phone = |psn: u64, kind: u64, data: Vec<(&str, bplist::Value)>| {
            let data = data.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            let command = bplist::encode(&bplist::Value::Dict(vec![
                ("type".into(), Int(kind)),
                ("data".into(), bplist::Value::Dict(data)),
            ]));
            framed(vec![
                ("type", Int(2)),
                ("ssn", Int(1)),
                ("psn", Int(psn)),
                ("data", bplist::Value::Data(command)),
            ])
        };
        sync.handle(&from_phone(
            3,
            START_LAYOUT_TRANSITION,
            vec![
                ("layoutID", S("0D38B526".into())),
                ("paletteID", S("starlight".into())),
            ],
        ));
        let palette = vec![
            ("layoutIdentifier", S("0D38B526".into())),
            ("paletteIdentifier", S("racingGreen".into())),
        ];
        sync.handle(&from_phone(4, PALETTE_CHANGE, palette));
        sync.handle(&from_phone(
            5,
            APPEARANCE_PREFERENCE_CHANGE,
            vec![("preference", Int(2))],
        ));
        sync.handle(&from_phone(
            6,
            APPEARANCE_PREFERENCE_CHANGE,
            vec![("preference", Int(7))],
        ));
        assert_eq!(
            sync.take_choices(),
            [
                PhoneChoice::Layout("0D38B526".into()),
                PhoneChoice::Palette {
                    layout: "0D38B526".into(),
                    palette: "starlight".into()
                },
                PhoneChoice::Palette {
                    layout: "0D38B526".into(),
                    palette: "racingGreen".into()
                },
                PhoneChoice::Appearance(Appearance::Dark),
            ]
        );
        assert!(sync.take_choices().is_empty());

        let asks = sync.handle(&from_phone(
            7,
            REQUEST_FOCUS,
            vec![("tokenID", Int(FOCUS_TOKEN))],
        ));
        assert_eq!(asks.len(), 2, "acked, then the focus goes back");
        let h = header_of(&asks[1]);
        let command =
            bplist::decode(h.get("data").and_then(bplist::Value::as_data).unwrap()).unwrap();
        assert_eq!(int(&command, "type"), Some(GIVE_FOCUS));

        let stale = framed(vec![("type", Int(2)), ("ssn", Int(9)), ("psn", Int(3))]);
        assert!(
            sync.handle(&stale).is_empty(),
            "another session is not ours"
        );
        assert!(sync.handle(&[0xff]).is_empty());

        sync.reopen();
        assert!(sync.handle(&reset).len() == 1);
        let out = sync.handle(&complete);
        let h = header_of(&out[2]);
        let command =
            bplist::decode(h.get("data").and_then(bplist::Value::as_data).unwrap()).unwrap();
        let data = command.get("data").unwrap();
        let layout = data.get("layoutID").and_then(bplist::Value::as_str);
        assert_eq!(
            layout,
            Some("DB7C6944"),
            "a new channel asks for the layout again"
        );
    }
}
