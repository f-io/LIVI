// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

const RESOLVE_INTERVAL: Duration = Duration::from_millis(500);

#[cfg(target_os = "linux")]
async fn ask_for_address(asked: &mut bool) {
    if *asked {
        return;
    }
    let Some(iface) = livi_link_host::link::host_iface() else {
        return;
    };
    *asked = true;
    let name = iface.clone();
    let taken = tokio::task::spawn_blocking(move || livi_link_host::link::take_address(&iface))
        .await
        .unwrap_or(false);
    if taken {
        println!(
            "[helper] LIVI Link on {name}: NetworkManager profile livi-link asks it for an address"
        );
    } else {
        println!("[helper] LIVI Link on {name}: no NetworkManager, the host has to address it");
    }
}

#[cfg(not(target_os = "linux"))]
async fn ask_for_address(_asked: &mut bool) {}

pub struct LinkPresence {
    on_bus: AtomicBool,
    present: AtomicBool,
    changed: Notify,
}

impl LinkPresence {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            on_bus: AtomicBool::new(false),
            present: AtomicBool::new(false),
            changed: Notify::new(),
        })
    }

    pub fn always() -> Arc<Self> {
        let link = Self::new();
        link.present.store(true, Ordering::SeqCst);
        link
    }

    pub fn set_on_bus(&self, on: bool) {
        self.on_bus.store(on, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    fn set_present(&self, present: bool) {
        self.present.store(present, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    pub fn is_present(&self) -> bool {
        self.present.load(Ordering::SeqCst)
    }

    pub fn changed(&self) -> &Notify {
        &self.changed
    }

    pub async fn wait_until(&self, present: bool) {
        self.wait_for(|l| l.is_present() == present).await;
    }

    async fn wait_for(&self, cond: impl Fn(&Self) -> bool) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if cond(self) {
                return;
            }
            notified.await;
        }
    }

    pub async fn resolve(
        self: Arc<Self>,
        on_up: impl Fn() + Send + 'static,
        on_down: impl Fn() + Send + 'static,
    ) {
        loop {
            self.wait_for(|l| l.on_bus.load(Ordering::SeqCst)).await;
            let mut asked = false;
            let up = loop {
                if !self.on_bus.load(Ordering::SeqCst) {
                    break false;
                }
                ask_for_address(&mut asked).await;
                if tokio::task::spawn_blocking(livi_link_host::link::resolves)
                    .await
                    .unwrap_or(false)
                {
                    break true;
                }
                tokio::time::sleep(RESOLVE_INTERVAL).await;
            };
            if !up {
                continue;
            }
            println!(
                "[helper] LIVI Link up: {} resolves",
                livi_link_host::link::LINK_NAME
            );
            on_up();
            self.set_present(true);
            self.wait_for(|l| !l.on_bus.load(Ordering::SeqCst)).await;
            self.set_present(false);
            on_down();
            println!("[helper] LIVI Link gone");
        }
    }
}
