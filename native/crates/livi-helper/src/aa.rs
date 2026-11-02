// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;

use livi_aa::usb::Control;
use livi_runtime::livi_sock::Broadcaster;

pub async fn watch_usb(control: Control, sessions: PathBuf, events: Broadcaster) {
    let subscribed = events.clone();
    livi_aa::usb::run(
        control,
        sessions,
        move |socket, peer, serial| {
            events.push_json(format!(
                "{{\"event\":\"aa-session\",\"socket\":\"{socket}\",\"peer\":\"{peer}\",\"transport\":\"usb\",\"serial\":\"{serial}\"}}"
            ));
        },
        async move { subscribed.subscribed().await },
    )
    .await;
}
