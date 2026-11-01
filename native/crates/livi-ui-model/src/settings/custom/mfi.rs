// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::message::{Action, MfiFile};
use serde_json::Value;

use crate::i18n::Strings;
use crate::settings::custom::files::Wanted;
use crate::settings::custom::info;
use crate::settings::view::{Local, Row, RowKind, Tool};

fn file_row(state: &Value, t: &Strings, file: MfiFile) -> Row {
    let (key, label, name, wanted) = match file {
        MfiFile::Certificate => (
            "certificate",
            t.t("settings.mfiCertificate", "Certificate"),
            "certificate.p7b",
            Wanted::Certificate,
        ),
        MfiFile::Identity => (
            "identity",
            t.t("settings.mfiIdentity", "Identity"),
            "identity.pk8",
            Wanted::Identity,
        ),
    };
    if state["mfi"][key].as_bool().unwrap_or(false) {
        return Row {
            kind: RowKind::Item,
            label,
            value: name.into(),
            enabled: true,
            tools: vec![Tool {
                icon: "Close",
                label: t.t("settings.remove", "Remove"),
                on: false,
                tint: None,
                enabled: true,
                action: Action::RemoveMfi { file },
                switch: false,
            }],
            ..Row::default()
        };
    }
    Row {
        kind: RowKind::Button,
        label,
        button: t.t("settings.import", "Import"),
        outlined: true,
        enabled: true,
        local: Some(Local::Browse(wanted)),
        ..Row::default()
    }
}

fn signing(state: &Value, t: &Strings) -> String {
    let mfi = &state["mfi"];
    if let Some(error) = mfi["error"].as_str() {
        return error.to_string();
    }
    let both =
        mfi["certificate"].as_bool() == Some(true) && mfi["identity"].as_bool() == Some(true);
    match mfi["generation"].as_u64() {
        Some(3) if both => t.t("settings.mfiFiles3", "Files, 3.0 (P-256)"),
        Some(2) if both => t.t("settings.mfiFiles2", "Files, 2.0 (RSA)"),
        _ => t.t("settings.mfiChip", "Coprocessor"),
    }
}

pub fn rows(state: &Value, t: &Strings) -> Vec<Row> {
    vec![
        file_row(state, t, MfiFile::Certificate),
        file_row(state, t, MfiFile::Identity),
        info(
            t.t("settings.mfiSigning", "Signing"),
            None,
            signing(state, t),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn missing_files_offer_import_and_the_chip_signs() {
        let t = Strings::default();
        let r = rows(&json!({ "mfi": {} }), &t);
        assert_eq!(r[0].button, "Import");
        assert_eq!(r[0].local, Some(Local::Browse(Wanted::Certificate)));
        assert_eq!(r[1].local, Some(Local::Browse(Wanted::Identity)));
        assert_eq!(r[0].action, None);
        assert_eq!(r[2].value, "Coprocessor");
    }

    #[test]
    fn imported_files_offer_remove_and_name_the_generation() {
        let t = Strings::default();
        let state = json!({ "mfi": { "certificate": true, "identity": true, "generation": 3 } });
        let r = rows(&state, &t);
        assert_eq!(r[0].kind, RowKind::Item);
        assert_eq!(r[0].value, "certificate.p7b");
        assert_eq!(r[1].tools.len(), 1);
        assert_eq!(r[1].tools[0].icon, "Close");
        assert_eq!(
            r[1].tools[0].action,
            Action::RemoveMfi {
                file: MfiFile::Identity
            }
        );
        assert_eq!(r[1].local, None);
        assert_eq!(r[2].value, "Files, 3.0 (P-256)");
        let half = json!({ "mfi": { "certificate": false, "identity": true, "generation": 2 } });
        assert_eq!(rows(&half, &t)[2].value, "Coprocessor");
        let broken = json!({ "mfi": { "identity": true, "error": "mfi files: bad key" } });
        assert_eq!(rows(&broken, &t)[2].value, "mfi files: bad key");
    }
}
