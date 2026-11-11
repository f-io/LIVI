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

use std::sync::{Arc, Mutex};

use livi_session_io::hotplug::{self, Event};
use nusb::{DeviceId, DeviceInfo};

pub const VENDOR: u16 = 0x1314;
pub const PRODUCTS: [u16; 2] = [0x1520, 0x1521];
pub const LINK_PRODUCT: &str = "LIVI Link";

pub type OnLink = dyn Fn(bool, &str) + Send + Sync;

pub fn on_air(wifi_interface: &str) -> Option<livi_wifi::OnAir> {
    if wifi_interface == link::CHOICE {
        ap::on_air()
    } else {
        livi_wifi::on_air(wifi_interface)
    }
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
