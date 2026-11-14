// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "android")]
extern crate nusb_android as nusb;

pub mod ap;
#[cfg(target_os = "linux")]
pub mod bt;
pub mod link;
#[cfg(any(target_os = "android", test))]
mod net4;
#[cfg(target_os = "android")]
pub mod usb_net;

#[cfg(usb_host)]
use std::sync::{Arc, Mutex};

#[cfg(usb_host)]
use livi_session_io::hotplug::{self, Event};
#[cfg(usb_host)]
use nusb::{DeviceId, DeviceInfo};

pub const VENDOR: u16 = 0x1314;
pub const PRODUCTS: [u16; 2] = [0x1520, 0x1521];
pub const LINK_PRODUCT: &str = "LIVI Link";

pub type OnLink = dyn Fn(bool, &str) + Send + Sync;

#[cfg(not(usb_host))]
const NETWORK_POLL: std::time::Duration = std::time::Duration::from_secs(2);

pub fn on_air(wifi_interface: &str) -> Option<livi_wifi::OnAir> {
    if wifi_interface == link::CHOICE {
        ap::on_air()
    } else {
        livi_wifi::on_air(wifi_interface)
    }
}

#[cfg(usb_host)]
pub fn is_livi_link(info: &DeviceInfo) -> bool {
    info.product_string() == Some(LINK_PRODUCT)
}

#[cfg(usb_host)]
pub fn is_dongle(info: &DeviceInfo) -> bool {
    info.vendor_id() == VENDOR && PRODUCTS.contains(&info.product_id())
}

#[cfg(usb_host)]
pub async fn run(on_link: impl Fn(bool, &str) + Send + Sync + 'static) {
    let on_link: Arc<OnLink> = Arc::new(on_link);
    let link: Arc<Mutex<Option<DeviceId>>> = Arc::default();
    hotplug::watch("dongle", |ev| match ev {
        Event::Present(info) | Event::Arrived(info) => seen(&link, &on_link, info),
        Event::Listed => {
            link::watching();
            println!("[dongle] watching for dongles");
        }
        Event::Left(id) => {
            let gone = {
                let mut l = link.lock().unwrap();
                *l == Some(id) && l.take().is_some()
            };
            if gone {
                println!("[dongle] LIVI Link left the bus");
                link::set_on_bus(false);
                on_link(false, "");
            }
        }
    })
    .await;
}

#[cfg(not(usb_host))]
pub async fn run(on_link: impl Fn(bool, &str) + Send + Sync + 'static) {
    link::watching();
    println!("[dongle] watching for the LIVI Link's network");
    let mut on = false;
    loop {
        if link::attached() != on {
            on = !on;
            link::set_on_bus(on);
            println!(
                "[dongle] LIVI Link {}",
                if on {
                    "on the network"
                } else {
                    "left the network"
                }
            );
            on_link(on, "");
        }
        tokio::time::sleep(NETWORK_POLL).await;
    }
}

#[cfg(usb_host)]
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
    link::set_on_bus(true);
    let serial = info.serial_number().unwrap_or("").to_owned();
    println!("[dongle] LIVI Link {serial} on the bus");
    on_link(true, &serial);
}
