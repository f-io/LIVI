pub mod accessory;
pub mod ap;
#[cfg(target_os = "linux")]
pub mod bt;
pub mod link;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use nusb::hotplug::HotplugEvent;
use nusb::{DeviceId, DeviceInfo};

pub const VENDOR: u16 = 0x1314;
pub const PRODUCTS: [u16; 2] = [0x1520, 0x1521];
pub const LINK_PRODUCT: &str = "LIVI Link";

const WATCH_RETRY: Duration = Duration::from_secs(5);

/// (on the bus, serial)
pub type OnLink = dyn Fn(bool, &str) + Send + Sync;

/// The access point a phone joins for wireless projection, whichever radio carries it.
pub fn on_air(wifi_interface: &str) -> Option<livi_wifi::OnAir> {
    if wifi_interface == link::CHOICE { ap::on_air() } else { livi_wifi::on_air(wifi_interface) }
}

pub fn is_livi_link(info: &DeviceInfo) -> bool {
    info.product_string() == Some(LINK_PRODUCT)
}

pub fn is_dongle(info: &DeviceInfo) -> bool {
    info.vendor_id() == VENDOR && PRODUCTS.contains(&info.product_id())
}

pub async fn run(on_link: impl Fn(bool, &str) + Send + Sync + 'static) {
    let on_link: Arc<OnLink> = Arc::new(on_link);
    let link: Arc<Mutex<Option<DeviceId>>> = Arc::default();
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
                seen(&link, &on_link, info);
            }
        }
        Err(e) => eprintln!("[dongle] list devices: {e}"),
    }
    println!("[dongle] watching for dongles");
    while let Some(ev) = watch.next().await {
        match ev {
            HotplugEvent::Connected(info) => seen(&link, &on_link, info),
            HotplugEvent::Disconnected(id) => {
                let gone = {
                    let mut l = link.lock().unwrap();
                    *l == Some(id) && l.take().is_some()
                };
                if gone {
                    println!("[dongle] LIVI Link left the bus");
                    on_link(false, "");
                }
            }
        }
    }
    eprintln!("[dongle] hotplug watch ended");
}

fn seen(link: &Mutex<Option<DeviceId>>, on_link: &Arc<OnLink>, info: DeviceInfo) {
    if !is_livi_link(&info) {
        return;
    }
    {
        let mut l = link.lock().unwrap();
        if l.is_some() {
            return;
        }
        *l = Some(info.id());
    }
    let serial = info.serial_number().unwrap_or("").to_owned();
    println!("[dongle] LIVI Link {serial} on the bus");
    on_link(true, &serial);
}
