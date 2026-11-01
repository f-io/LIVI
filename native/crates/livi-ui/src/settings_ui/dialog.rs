// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) fn show_dialog(c: &mut Controller, s: &SettingsState, dialog: Option<Dialog>) {
    let Some(d) = dialog else {
        c.dismissed = None;
        c.dialog = None;
        return;
    };
    if c.dismissed.as_deref() == Some(d.key.as_str()) {
        c.dialog = None;
        return;
    }
    c.dismissed = None;
    let mut at = 0;
    let data: Vec<RowData> = d
        .rows
        .iter()
        .map(|r| {
            let focus = if r.kind == ModelRowKind::Button && r.enabled {
                at += 1;
                at - 1
            } else {
                -1
            };
            row_data(r, focus)
        })
        .collect();
    s.set_dialog_title(d.title.clone().into());
    s.set_dialog_rows(ModelRc::from(Rc::new(VecModel::from(data))));
    s.set_dialog_open(true);
    c.dialog = Some(d);
}

pub fn dialog_buttons(s: &SettingsState) -> Vec<usize> {
    if !s.get_dialog_open() {
        return Vec::new();
    }
    let rows = s.get_dialog_rows();
    (0..rows.row_count())
        .filter(|&i| {
            rows.row_data(i)
                .is_some_and(|r| r.kind == RowKind::Button && r.enabled)
        })
        .collect()
}
