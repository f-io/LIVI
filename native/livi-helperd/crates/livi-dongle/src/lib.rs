// CarlinKit dongle on USB.

pub mod link;
pub mod session;
pub mod wire;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use std::time::Duration;

use futures_util::StreamExt;
use livi_session_io::usb::{UsbStream, open_pipe};
use nusb::hotplug::HotplugEvent;
use nusb::{DeviceId, DeviceInfo};

pub const VENDOR: u16 = 0x1314;
pub const PRODUCTS: [u16; 2] = [0x1520, 0x1521];

const WATCH_RETRY: Duration = Duration::from_secs(5);
const REOFFER: Duration = Duration::from_millis(300);

/// What the main process hears about a session, besides the socket path.
pub struct Announce {
    pub serial: String,
    pub product: u16,
    pub version: u16,
    pub name: String,
}

pub type OnSession = dyn Fn(&str, &Announce) + Send + Sync;
/// Presence of the LIVI Link dongle: on the bus, and its serial.
pub type OnLink = dyn Fn(bool, &str) + Send + Sync;

#[derive(Default)]
struct Dongles {
    present: HashSet<DeviceId>,
    busy: HashSet<DeviceId>,
    /// Per device, notified when it is unplugged so its session ends promptly.
    cancel: HashMap<DeviceId, Arc<Notify>>,
    /// The LIVI Link dongle, while it is on the bus.
    link: Option<DeviceId>,
}

type Shared = Arc<Mutex<Dongles>>;

pub fn is_dongle(info: &DeviceInfo) -> bool {
    info.vendor_id() == VENDOR && PRODUCTS.contains(&info.product_id())
}

/// The product string a dongle carries once provisioned as LIVI Link (an NCM bridge).
pub const LINK_PRODUCT: &str = "LIVI Link";

pub fn is_livi_link(info: &DeviceInfo) -> bool {
    is_dongle(info) && info.product_string() == Some(LINK_PRODUCT)
}

pub async fn run(
    on_link: impl Fn(bool, &str) + Send + Sync + 'static,
    on_session: impl Fn(&str, &Announce) + Send + Sync + 'static,
) {
    let on_link: Arc<OnLink> = Arc::new(on_link);
    let on_session: Arc<OnSession> = Arc::new(on_session);
    let dongles: Shared = Arc::default();
    let mut watch = loop {
        match nusb::watch_devices() {
            Ok(w) => break w,
            Err(e) => {
                eprintln!("[dongle] hotplug watch: {e}, retrying");
                tokio::time::sleep(WATCH_RETRY).await;
            }
        }
    };
    match nusb::list_devices().await {
        Ok(list) => {
            for info in list {
                seen(&dongles, &on_link, &on_session, info);
            }
        }
        Err(e) => eprintln!("[dongle] list devices: {e}"),
    }
    println!("[dongle] watching for dongles");
    while let Some(ev) = watch.next().await {
        match ev {
            HotplugEvent::Connected(info) => seen(&dongles, &on_link, &on_session, info),
            HotplugEvent::Disconnected(id) => {
                let link_gone = {
                    let mut d = dongles.lock().unwrap();
                    d.present.remove(&id);
                    if let Some(c) = d.cancel.get(&id) {
                        c.notify_waiters();
                    }
                    d.link == Some(id) && d.link.take().is_some()
                };
                if link_gone {
                    println!("[dongle] LIVI Link left the bus");
                    on_link(false, "");
                }
            }
        }
    }
    eprintln!("[dongle] hotplug watch ended");
}

fn seen(dongles: &Shared, on_link: &Arc<OnLink>, on_session: &Arc<OnSession>, info: DeviceInfo) {
    if is_livi_link(&info) {
        let serial = info.serial_number().unwrap_or("").to_owned();
        {
            let mut d = dongles.lock().unwrap();
            if d.link.is_some() {
                return;
            }
            d.link = Some(info.id());
        }
        println!("[dongle] LIVI Link {serial} on the bus");
        on_link(true, &serial);
        return;
    }
    if !is_dongle(&info) {
        return;
    }
    let cancel = {
        let mut d = dongles.lock().unwrap();
        d.present.insert(info.id());
        if !d.busy.insert(info.id()) {
            return;
        }
        let c = Arc::new(Notify::new());
        d.cancel.insert(info.id(), c.clone());
        c
    };
    let dongles = dongles.clone();
    let on_session = on_session.clone();
    tokio::spawn(async move {
        serve(&dongles, &on_session, &info, cancel).await;
        let mut d = dongles.lock().unwrap();
        d.busy.remove(&info.id());
        d.cancel.remove(&info.id());
    });
}

/// Serves one session after another for as long as the dongle stays on the bus.
async fn serve(dongles: &Shared, on_session: &Arc<OnSession>, info: &DeviceInfo, cancel: Arc<Notify>) {
    let announce = Announce {
        serial: info.serial_number().unwrap_or("").to_owned(),
        product: info.product_id(),
        version: info.device_version(),
        name: info.product_string().unwrap_or("").to_owned(),
    };
    let label = if announce.serial.is_empty() {
        "dongle".to_owned()
    } else {
        format!("dongle:{}", announce.serial)
    };
    loop {
        let dev = match info.open().await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[dongle] {label}: open: {e}");
                return;
            }
        };
        let pipe = match open_pipe(&dev).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[dongle] {label}: {e}");
                return;
            }
        };
        let Some((node, path)) = livi_session_io::sock::bind_session_socket("dongle-session") else { return };
        println!("[dongle] {label}: session socket {path}");
        on_session(&path, &announce);
        let end = session::run(UsbStream::new(pipe), &label, node, path, cancel.clone()).await;
        if end.reset
            && let Err(e) = dev.reset().await
        {
            eprintln!("[dongle] {label}: reset: {e}");
        }
        drop(dev);
        println!("[dongle] {label}: session ended{}", if end.by_node { " by the main process" } else { "" });
        tokio::select! {
            _ = tokio::time::sleep(REOFFER) => {}
            _ = cancel.notified() => return,
        }
        if !dongles.lock().unwrap().present.contains(&info.id()) {
            return;
        }
    }
}
