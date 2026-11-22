// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! TELEMETRY.md at the repository root is written from this crate's sources, a stale copy fails
//! the test. `LIVI_WRITE_DOCS=1 cargo test -p livi-core-proto` writes it anew. A field's values
//! come from a `/// Values:` line on the field or on its struct, a type name there lists that
//! type's names.

use std::path::Path;

const TELEMETRY: &str = include_str!("telemetry.rs");
const GNSS: &str = include_str!("gnss.rs");
const CONFIG: &str = include_str!("config.rs");

const FLATTENED: [&str; 8] = [
    "GpsFix",
    "TirePressure",
    "Doors",
    "SeatBelts",
    "Telltales",
    "CanFrame",
    "Climate",
    "Notification",
];

const UNITS: [(&str, &str); 15] = [
    ("LPer100Km", "l/100 km"),
    ("Kph", "km/h"),
    ("Kpa", "kPa"),
    ("Kwh", "kWh"),
    ("Kw", "kW"),
    ("Lph", "l/h"),
    ("Pct", "%"),
    ("Km", "km"),
    ("Deg", "°"),
    ("Lux", "lx"),
    ("Ms", "m/s"),
    ("Ts", "Unix ms"),
    ("C", "°C"),
    ("V", "V"),
    ("M", "m"),
];

struct Field {
    name: String,
    ty: String,
    values: Option<String>,
}

fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn body<'a>(source: &'a str, item: &str) -> Option<&'a str> {
    let head = format!("{item} {{");
    let rest = &source[source.find(&head)? + head.len()..];
    Some(&rest[..rest.find("\n}")?])
}

fn fields(source: &str, name: &str) -> Vec<Field> {
    let body =
        body(source, &format!("pub struct {name}")).unwrap_or_else(|| panic!("{name} is gone"));
    let head = &source[..source.find(&format!("pub struct {name} {{")).unwrap_or(0)];
    let shared = head
        .lines()
        .rev()
        .take_while(|l| l.starts_with("///") || l.starts_with("#["))
        .find_map(|l| l.strip_prefix("/// Values: "));
    let mut out = Vec::new();
    let mut values = None;
    for line in body.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("/// Values: ") {
            values = Some(v.to_string());
        } else if let Some((name, ty)) = line
            .strip_prefix("pub ")
            .and_then(|l| l.trim_end_matches(',').split_once(": "))
        {
            out.push(Field {
                name: camel(name),
                ty: ty.trim().to_string(),
                values: values.take().or_else(|| shared.map(String::from)),
            });
        }
    }
    out
}

/// The names a type takes: its `ALL` list when it has one, else its variants as serde names them.
fn names(ty: &str) -> Option<Vec<String>> {
    for source in [CONFIG, TELEMETRY] {
        if let Some(all) = body(source, &format!("impl {ty}"))
            .and_then(|b| b.split_once("const ALL"))
            .map(|(_, all)| all)
        {
            let list = &all[..all.find("];").unwrap_or(all.len())];
            return Some(
                list.split('"')
                    .skip(1)
                    .step_by(2)
                    .map(String::from)
                    .collect(),
            );
        }
        if let Some(body) = body(source, &format!("pub enum {ty}")) {
            let lower_first = |v: &str| {
                let mut c = v.chars();
                c.next()
                    .map(|f| f.to_lowercase().chain(c).collect())
                    .unwrap_or_default()
            };
            return Some(
                body.lines()
                    .map(|l| l.trim().trim_end_matches(','))
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(|l| lower_first(l.split(' ').next().unwrap_or(l)))
                    .collect(),
            );
        }
    }
    None
}

fn inner(ty: &str) -> &str {
    ty.strip_prefix("Option<")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(ty)
}

fn kind(ty: &str) -> &'static str {
    match inner(ty) {
        "f64" | "u8" | "u32" | "u64" => "number",
        "bool" => "bool",
        "String" => "string",
        "Value" => "number or string",
        "Vec<String>" => "list of strings",
        "Vec<u8>" => "list of numbers",
        other if names(other).is_some() => "string",
        _ => "object",
    }
}

fn shown(field: &Field) -> String {
    let listed = |names: Vec<String>| {
        names
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match &field.values {
        Some(v) => names(v).map_or_else(|| v.clone(), listed),
        None if inner(&field.ty) == "bool" => "`true`, `false`".to_string(),
        None => names(inner(&field.ty)).map_or_else(String::new, listed),
    }
}

fn unit(name: &str) -> &'static str {
    let last = name.rsplit('.').next().unwrap_or(name);
    match last {
        "lat" | "lng" | "heading" => "°",
        "alt" => "m",
        "ts" => "Unix ms",
        "rpm" => "1/min",
        _ => UNITS
            .iter()
            .find(|(suffix, _)| last.ends_with(suffix))
            .map_or("", |(_, unit)| unit),
    }
}

fn rows(source: &str, name: &str, prefix: &str, out: &mut Vec<[String; 4]>) {
    for field in fields(source, name) {
        let path = format!("{prefix}{}", field.name);
        let nested = inner(&field.ty);
        if FLATTENED.contains(&nested) {
            let source = if body(TELEMETRY, &format!("pub struct {nested}")).is_some() {
                TELEMETRY
            } else {
                GNSS
            };
            rows(source, nested, &format!("{path}."), out);
            continue;
        }
        out.push([
            format!("`{path}`"),
            kind(&field.ty).to_string(),
            unit(&path).to_string(),
            shown(&field),
        ]);
    }
}

/// Padded the way dprint lays out a table, else the lint rewrites the file.
fn table(rows: &[[String; 4]]) -> String {
    let width = |c: usize| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0);
    let widths = [0, 1, 2, 3].map(width);
    let line = |cells: [&str; 4]| {
        let padded = (0..4).map(|c| format!("{:w$}", cells[c], w = widths[c]));
        format!("| {} |\n", padded.collect::<Vec<_>>().join(" | "))
    };
    let rule = widths.map(|w| "-".repeat(w));
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        out.push_str(&line(row.each_ref().map(String::as_str)));
        if i == 0 {
            out.push_str(&line(rule.each_ref().map(String::as_str)));
        }
    }
    out
}

fn document() -> String {
    let mut doc = String::from(
        "# Telemetry\n\n\
         A push carries any subset of the fields below, a dot names a field inside a group:\n\n\
         - Socket.IO: `io('ws://livi.local:4000').emit('telemetry:push', { speedKph: 73 })`\n\
         - HTTP: `curl -d '{\"speedKph\":73}' http://livi.local:4000/telemetry`\n\
         - Shell: `livi-core telemetry set speedKph=73 gps.lat=53.59`\n\n\
         Which receiver takes which field is listed in \
         [telemetry.rs](native/crates/livi-core-proto/src/telemetry.rs).\n\n",
    );
    let mut table_rows = vec![["Field", "Type", "Unit", "Values"].map(String::from)];
    rows(TELEMETRY, "TelemetrySnapshot", "", &mut table_rows);
    doc.push_str(&table(&table_rows));
    doc.push_str(
        "\n## Keys\n\n\
         `key` takes the name of a key binding, case sensitive. While carplay or android auto is \
         in front the key goes to the phone, while LIVI is in front the navigation keys move LIVI's \
         UI.\n\n",
    );
    for field in fields(CONFIG, "KeyBindings") {
        doc.push_str(&format!("- `{}`\n", field.name));
    }
    doc.push_str(
        "\n---\n\nWritten from `native/crates/livi-core-proto`, `LIVI_WRITE_DOCS=1 cargo test -p \
         livi-core-proto`\n",
    );
    doc
}

#[test]
fn telemetry_md_is_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../TELEMETRY.md");
    let doc = document();
    if std::env::var_os("LIVI_WRITE_DOCS").is_some() {
        std::fs::write(&path, &doc).unwrap();
        return;
    }
    let kept = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        kept == doc,
        "TELEMETRY.md is stale, LIVI_WRITE_DOCS=1 cargo test -p livi-core-proto writes it anew"
    );
}
