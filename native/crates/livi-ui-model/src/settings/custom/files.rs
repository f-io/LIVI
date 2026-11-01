// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::i18n::Strings;
use crate::settings::view::{Local, Row, RowKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    Picture,
    Certificate,
    Identity,
}

impl Wanted {
    fn endings(self) -> &'static [&'static str] {
        match self {
            Wanted::Picture => &[".png", ".jpg", ".jpeg"],
            Wanted::Certificate => &[".p7b"],
            Wanted::Identity => &[".pk8"],
        }
    }

    pub fn accepts(self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        self.endings().iter().any(|e| lower.ends_with(e))
    }

    pub fn title(self, t: &Strings) -> String {
        match self {
            Wanted::Picture => t.t("settings.importPng", "Import PNG"),
            Wanted::Certificate => t.t("settings.mfiCertificate", "Certificate"),
            Wanted::Identity => t.t("settings.mfiIdentity", "Identity"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Wanted::Picture => "ImageOutlined",
            Wanted::Certificate => "VerifiedUserOutlined",
            Wanted::Identity => "KeyOutlined",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

pub fn listing(entries: Vec<Entry>, wanted: Wanted) -> Vec<Entry> {
    let mut out: Vec<Entry> = entries
        .into_iter()
        .filter(|e| !e.name.starts_with('.') && (e.dir || wanted.accepts(&e.name)))
        .collect();
    out.sort_by_cached_key(|e| (!e.dir, e.name.to_lowercase()));
    out
}

pub fn entry_rows(entries: &[Entry], wanted: Wanted, t: &Strings) -> Vec<Row> {
    if entries.is_empty() {
        return vec![Row {
            kind: RowKind::Info,
            label: t.t("settings.noFiles", "No matching files here"),
            enabled: true,
            ..Row::default()
        }];
    }
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| Row {
            kind: RowKind::Pick,
            label: e.name.clone(),
            icon: Some(if e.dir {
                "FolderOutlined"
            } else {
                wanted.icon()
            }),
            enabled: true,
            local: Some(Local::PickEntry(i)),
            ..Row::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, dir: bool) -> Entry {
        Entry {
            name: name.into(),
            dir,
        }
    }

    #[test]
    fn listing_keeps_folders_first_and_only_wanted_files() {
        let all = vec![
            entry("b.PNG", false),
            entry("notes.txt", false),
            entry(".hidden", true),
            entry("Zeta", true),
            entry("a.jpeg", false),
            entry("certificate.p7b", false),
            entry("identity.pk8", false),
            entry("alpha", true),
        ];
        assert_eq!(
            listing(all.clone(), Wanted::Picture),
            vec![
                entry("alpha", true),
                entry("Zeta", true),
                entry("a.jpeg", false),
                entry("b.PNG", false),
            ]
        );
        assert_eq!(
            listing(all.clone(), Wanted::Certificate),
            vec![
                entry("alpha", true),
                entry("Zeta", true),
                entry("certificate.p7b", false)
            ]
        );
        assert_eq!(
            listing(all, Wanted::Identity),
            vec![
                entry("alpha", true),
                entry("Zeta", true),
                entry("identity.pk8", false)
            ]
        );
    }

    #[test]
    fn entry_rows_point_at_their_entry() {
        let t = Strings::default();
        let rows = entry_rows(
            &[entry("keys", true), entry("identity.pk8", false)],
            Wanted::Identity,
            &t,
        );
        assert_eq!(rows[0].icon, Some("FolderOutlined"));
        assert_eq!(rows[1].icon, Some("KeyOutlined"));
        assert_eq!(rows[1].local, Some(Local::PickEntry(1)));
        assert_eq!(entry_rows(&[], Wanted::Picture, &t)[0].kind, RowKind::Info);
    }
}
