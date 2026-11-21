// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::Duration;

use futures_util::StreamExt;
use nusb::hotplug::HotplugEvent;
use nusb::{DeviceId, DeviceInfo};

const WATCH_RETRY: Duration = Duration::from_secs(5);
#[cfg(target_os = "linux")]
const POLL: Duration = Duration::from_secs(2);

pub enum Event {
    Present(DeviceInfo),
    Listed,
    Arrived(DeviceInfo),
    Left(DeviceId),
}

pub async fn watch(tag: &str, mut on: impl FnMut(Event)) {
    #[cfg(target_os = "linux")]
    if !std::path::Path::new("/run/udev/control").exists() {
        return poll(tag, on).await;
    }
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

/// nusb's hotplug stream is udevd's rebroadcast, silent without udevd.
#[cfg(target_os = "linux")]
async fn poll(tag: &str, mut on: impl FnMut(Event)) {
    let mut known: Vec<DeviceId> = Vec::new();
    let mut listed = false;
    loop {
        match nusb::list_devices().await {
            Ok(list) => {
                let list: Vec<DeviceInfo> = list.collect();
                known.retain(|id| {
                    let here = list.iter().any(|info| info.id() == *id);
                    if !here {
                        on(Event::Left(*id));
                    }
                    here
                });
                for info in list {
                    if !known.contains(&info.id()) {
                        known.push(info.id());
                        on(if listed {
                            Event::Arrived(info)
                        } else {
                            Event::Present(info)
                        });
                    }
                }
            }
            Err(e) => eprintln!("[{tag}] list devices: {e}"),
        }
        if !listed {
            listed = true;
            on(Event::Listed);
        }
        tokio::time::sleep(POLL).await;
    }
}
