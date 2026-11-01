// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) fn shown(v: &Value) -> std::string::String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Real(f) => f.to_string(),
        Value::String(s) => s.clone(),
        Value::Data(d) => format!("{} B", d.len()),
        Value::Array(items) if items.len() > 4 => format!("{} items", items.len()),
        other => format!("{other:?}"),
    }
}

pub(super) fn update_notify(changed: Vec<(Value, Value)>) -> Option<(Lane, Vec<u8>)> {
    (!changed.is_empty()).then(|| {
        let update = dict([
            ("command", Value::str("updateNotify")),
            ("values", Value::Dict(changed)),
        ]);
        (Lane::High, wrap(&update))
    })
}

pub(super) fn wrap(inner: &Value) -> Vec<u8> {
    opack::encode(&dict([
        ("pluginID", Value::Int(PLUGIN_ID)),
        ("pluginData", Value::Data(opack::encode(inner))),
    ]))
}

pub(super) fn to_plist(v: &Value) -> bplist::Value {
    match v {
        Value::Null => bplist::Value::Data(Vec::new()),
        Value::Bool(b) => bplist::Value::Bool(*b),
        Value::Int(n) => bplist::Value::Int(u64::try_from(*n).unwrap_or(0)),
        Value::Real(f) => bplist::Value::Real(*f),
        Value::String(s) => bplist::Value::String(s.clone()),
        Value::Data(d) => bplist::Value::Data(d.clone()),
        Value::Uuid(u) => bplist::Value::Data(u.to_vec()),
        Value::Array(items) => bplist::Value::Array(items.iter().map(to_plist).collect()),
        Value::Dict(entries) => bplist::Value::Dict(
            entries
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        Value::String(s) => s.clone(),
                        Value::Int(n) => n.to_string(),
                        other => format!("{other:?}"),
                    };
                    (key, to_plist(v))
                })
                .collect(),
        ),
    }
}
