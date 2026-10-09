use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(target_os = "linux")]
use livi_runtime::bringup::OnCable;
use livi_runtime::livi_sock::Broadcaster;
#[cfg(target_os = "linux")]
use livi_runtime::state::HelperState;
use tokio::sync::Notify;

const RESOLVE_INTERVAL: Duration = Duration::from_millis(500);
const WATCH_RETRY: Duration = Duration::from_secs(2);

/// CarPlay over the cable keeps the phone's Bluetooth to the accessory disconnected.
#[cfg(target_os = "linux")]
pub fn drop_dongle_link(mac: String) {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = livi_link_host::accessory::drop_link(&mac) {
            eprintln!("[helperd] {mac} stays on the dongle's bluetooth: {e}");
        }
    });
}

#[cfg(target_os = "linux")]
pub fn dongle_on_cable(state: Arc<HelperState>) -> OnCable {
    OnCable(Arc::new(move |mac: &str| {
        let cabled = state.carkit_claims(mac);
        if cabled {
            drop_dongle_link(mac.to_string());
        }
        cabled
    }))
}

/// LIVI ends a CarPlay session as soon as its phone leaves the access point.
pub async fn relay_stations(bcast: Broadcaster) {
    loop {
        let events = bcast.clone();
        let _ = tokio::task::spawn_blocking(move || {
            livi_link_host::ap::watch_stations(|joined, mac| {
                let event = if joined { "joined" } else { "left" };
                println!("[helperd] {mac} {event} the dongle's access point");
                events.push_json(format!(
                    "{{\"type\":\"wifi\",\"event\":\"{event}\",\"mac\":\"{mac}\"}}"
                ));
            })
        })
        .await;
        tokio::time::sleep(WATCH_RETRY).await;
    }
}

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

    /// The MFi chip is on this machine: nothing to wait for.
    #[cfg(target_os = "linux")]
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
            let up = loop {
                if !self.on_bus.load(Ordering::SeqCst) {
                    break false;
                }
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
            println!("[helperd] LIVI Link up: {} resolves", livi_link_host::link::LINK_NAME);
            on_up();
            self.set_present(true);
            self.wait_for(|l| !l.on_bus.load(Ordering::SeqCst)).await;
            self.set_present(false);
            on_down();
            println!("[helperd] LIVI Link gone");
        }
    }
}
