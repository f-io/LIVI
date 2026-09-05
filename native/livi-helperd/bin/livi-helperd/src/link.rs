//! The LIVI Link as the helper sees it: on the bus or not, and once its name resolves,
//! present. What needs the dongle waits on it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

const RESOLVE_INTERVAL: Duration = Duration::from_millis(500);

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

    /// Returns once the link is in the wanted state.
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

    /// While the dongle is on the bus, waits for its name to resolve; reports the link up
    /// then and down when the dongle leaves.
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
                if tokio::task::spawn_blocking(livi_dongle::link::resolves).await.unwrap_or(false) {
                    break true;
                }
                tokio::time::sleep(RESOLVE_INTERVAL).await;
            };
            if !up {
                continue;
            }
            println!("[helperd] LIVI Link up: {} resolves", livi_dongle::link::LINK_NAME);
            on_up();
            self.set_present(true);
            self.wait_for(|l| !l.on_bus.load(Ordering::SeqCst)).await;
            self.set_present(false);
            on_down();
            println!("[helperd] LIVI Link gone");
        }
    }
}
