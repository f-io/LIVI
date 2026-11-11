// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::Duration;

use futures_util::StreamExt;
use nusb::hotplug::HotplugEvent;
use nusb::{DeviceId, DeviceInfo};

const WATCH_RETRY: Duration = Duration::from_secs(5);

pub enum Event {
    Present(DeviceInfo),
    Listed,
    Arrived(DeviceInfo),
    Left(DeviceId),
}

pub async fn watch(tag: &str, mut on: impl FnMut(Event)) {
    let mut watch = loop {
        match nusb::watch_devices() {
            Ok(w) => break w,
            Err(e) => {
                eprintln!("[{tag}] hotplug watch: {e}, retrying");
                tokio::time::sleep(WATCH_RETRY).await;
            }
        }
    };
    match nusb::list_devices().await {
        Ok(list) => list.for_each(|info| on(Event::Present(info))),
        Err(e) => eprintln!("[{tag}] list devices: {e}"),
    }
    on(Event::Listed);
    while let Some(ev) = watch.next().await {
        on(match ev {
            HotplugEvent::Connected(info) => Event::Arrived(info),
            HotplugEvent::Disconnected(id) => Event::Left(id),
        });
    }
    eprintln!("[{tag}] hotplug watch ended");
}
