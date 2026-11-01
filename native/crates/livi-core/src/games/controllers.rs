// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::time::Duration;

use livi_core_proto::state::ControllerView;
use zbus::{Connection, OwnedObjectPath, OwnedVariant, Variant};

const SCAN_FOR: Duration = Duration::from_secs(8);
const PAIR_WITHIN: Duration = Duration::from_secs(45);
const TRUST_WITHIN: Duration = Duration::from_secs(10);
const CONNECT_WITHIN: Duration = Duration::from_secs(15);
const HID_UUIDS: [&str; 2] = ["00001124", "00001812"];
const NAME_HINTS: [&str; 10] = [
    "controller",
    "gamepad",
    "dualshock",
    "dualsense",
    "xbox",
    "8bitdo",
    "joycon",
    "joy-con",
    "pro controller",
    "wireless controller",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    pub mac: String,
    pub name: String,
    pub alias: String,
    pub icon: String,
    pub uuids: Vec<String>,
    pub paired: bool,
    pub connected: bool,
}

pub fn valid_mac(mac: &str) -> Result<String, String> {
    let value = mac.trim().to_uppercase();
    let ok = value.len() == 17
        && value.split(':').count() == 6
        && value
            .split(':')
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()));
    if ok {
        Ok(value)
    } else {
        Err("Invalid Bluetooth address".into())
    }
}

fn is_controller(seen: &Seen, name: &str) -> bool {
    let lower = name.to_lowercase();
    seen.icon == "input-gaming"
        || seen
            .uuids
            .iter()
            .any(|u| HID_UUIDS.iter().any(|h| u.to_lowercase().starts_with(h)))
        || NAME_HINTS.iter().any(|h| lower.contains(h))
}

pub fn pick(seen: Vec<Seen>) -> Vec<ControllerView> {
    let mut out: Vec<ControllerView> = seen
        .into_iter()
        .filter_map(|s| {
            let name = [&s.name, &s.alias]
                .into_iter()
                .find(|n| !n.trim().is_empty())
                .cloned()
                .unwrap_or_default();
            (s.paired || is_controller(&s, &name)).then(|| ControllerView {
                mac: s.mac.to_uppercase(),
                name,
                paired: s.paired,
                connected: s.connected,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.mac.cmp(&b.mac))
    });
    out
}

pub fn device_path(adapter: &str, mac: &str) -> String {
    format!("/org/bluez/{adapter}/dev_{}", mac.replace(':', "_"))
}

pub trait Bus {
    async fn pair(&self, device: &str) -> Result<(), String>;
    async fn trust(&self, device: &str) -> Result<(), String>;
    async fn connect(&self, device: &str) -> Result<(), String>;
}

pub async fn pair_with(bus: &impl Bus, adapter: &str, mac: &str) -> Result<(), String> {
    let device = device_path(adapter, &valid_mac(mac)?);
    bus.pair(&device).await?;
    bus.trust(&device).await?;
    // A sleeping controller may refuse the first connect, it is paired all the same.
    if let Err(e) = bus.connect(&device).await {
        println!("[games] {mac} paired, not connected yet: {e}");
    }
    Ok(())
}

pub struct Bluez {
    conn: Connection,
}

fn already(e: &zbus::Error) -> bool {
    matches!(e, zbus::Error::MethodError(name, _, _) if name.as_str() == "org.bluez.Error.AlreadyExists")
}

async fn within<T>(
    limit: Duration,
    call: impl Future<Output = zbus::Result<T>>,
) -> Result<T, String> {
    tokio::time::timeout(limit, call)
        .await
        .map_err(|_| "Bluetooth did not answer in time".to_string())?
        .map_err(|e| e.to_string())
}

impl Bus for Bluez {
    async fn pair(&self, device: &str) -> Result<(), String> {
        let call = self.conn.call_method(
            Some("org.bluez"),
            device,
            Some("org.bluez.Device1"),
            "Pair",
            &(),
        );
        match tokio::time::timeout(PAIR_WITHIN, call).await {
            Err(_) => Err("pairing timed out".into()),
            Ok(Err(e)) if already(&e) => Ok(()),
            Ok(r) => r.map(|_| ()).map_err(|e| e.to_string()),
        }
    }

    async fn trust(&self, device: &str) -> Result<(), String> {
        within(
            TRUST_WITHIN,
            self.conn.call_method(
                Some("org.bluez"),
                device,
                Some("org.freedesktop.DBus.Properties"),
                "Set",
                &("org.bluez.Device1", "Trusted", Variant::from(true)),
            ),
        )
        .await
        .map(|_| ())
    }

    async fn connect(&self, device: &str) -> Result<(), String> {
        within(
            CONNECT_WITHIN,
            self.conn.call_method(
                Some("org.bluez"),
                device,
                Some("org.bluez.Device1"),
                "Connect",
                &(),
            ),
        )
        .await
        .map(|_| ())
    }
}

type Props = HashMap<String, OwnedVariant>;

fn prop<T: TryFrom<OwnedVariant>>(props: &Props, key: &str) -> Option<T> {
    props
        .get(key)?
        .try_clone()
        .ok()
        .and_then(|v| T::try_from(v).ok())
}

impl Bluez {
    pub async fn open() -> Result<Self, String> {
        if !cfg!(target_os = "linux") {
            return Err("Bluetooth controller pairing requires Linux".into());
        }
        let conn = Connection::system()
            .await
            .map_err(|e| format!("Bluetooth is not available: {e}"))?;
        Ok(Self { conn })
    }

    pub async fn list(&self, adapter: &str) -> Result<Vec<ControllerView>, String> {
        let reply = within(
            Duration::from_secs(5),
            self.conn.call_method(
                Some("org.bluez"),
                "/",
                Some("org.freedesktop.DBus.ObjectManager"),
                "GetManagedObjects",
                &(),
            ),
        )
        .await?;
        let objects: HashMap<OwnedObjectPath, HashMap<String, Props>> =
            reply.body().deserialize().map_err(|e| e.to_string())?;
        let prefix = format!("/org/bluez/{adapter}/dev_");
        let seen = objects
            .into_iter()
            .filter(|(path, _)| path.as_str().starts_with(&prefix))
            .filter_map(|(_, mut ifaces)| ifaces.remove("org.bluez.Device1"))
            .map(|p| Seen {
                mac: prop(&p, "Address").unwrap_or_default(),
                name: prop(&p, "Name").unwrap_or_default(),
                alias: prop(&p, "Alias").unwrap_or_default(),
                icon: prop(&p, "Icon").unwrap_or_default(),
                uuids: prop(&p, "UUIDs").unwrap_or_default(),
                paired: prop(&p, "Paired").unwrap_or(false),
                connected: prop(&p, "Connected").unwrap_or(false),
            })
            .filter(|s| !s.mac.is_empty())
            .collect();
        Ok(pick(seen))
    }

    pub async fn scan(&self, adapter: &str) -> Result<Vec<ControllerView>, String> {
        let path = format!("/org/bluez/{adapter}");
        let adapter_call = |method: &'static str| {
            self.conn.call_method(
                Some("org.bluez"),
                path.as_str(),
                Some("org.bluez.Adapter1"),
                method,
                &(),
            )
        };
        let started = within(Duration::from_secs(5), adapter_call("StartDiscovery")).await;
        if let Err(e) = &started
            && !e.contains("InProgress")
        {
            return Err(e.clone());
        }
        // BlueZ keeps what it found in its object tree for the list below.
        tokio::time::sleep(SCAN_FOR).await;
        if started.is_ok() {
            let _ = within(Duration::from_secs(5), adapter_call("StopDiscovery")).await;
        }
        self.list(adapter).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn controllers_and_paired_devices_are_listed_connected_first() {
        let pad = Seen {
            mac: "aa:bb:cc:dd:ee:ff".into(),
            name: "8BitDo Pro 2".into(),
            icon: "input-gaming".into(),
            ..Default::default()
        };
        let phone = Seen {
            mac: "11:22:33:44:55:66".into(),
            name: "Pixel".into(),
            icon: "phone".into(),
            ..Default::default()
        };
        assert_eq!(
            pick(vec![pad.clone(), phone.clone()]),
            vec![ControllerView {
                mac: "AA:BB:CC:DD:EE:FF".into(),
                name: "8BitDo Pro 2".into(),
                paired: false,
                connected: false,
            }]
        );
        let hid = Seen {
            mac: "22:22:22:22:22:22".into(),
            alias: "Unnamed".into(),
            uuids: vec!["00001124-0000-1000-8000-00805f9b34fb".into()],
            ..Default::default()
        };
        let by_name = Seen {
            mac: "33:33:33:33:33:33".into(),
            name: "Xbox Wireless Controller".into(),
            connected: true,
            ..Default::default()
        };
        let paired_phone = Seen {
            paired: true,
            ..phone
        };
        let names: Vec<_> = pick(vec![pad, paired_phone, hid, by_name])
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(
            names,
            [
                "Xbox Wireless Controller",
                "8BitDo Pro 2",
                "Pixel",
                "Unnamed"
            ]
        );
    }

    #[test]
    fn only_real_addresses_are_accepted() {
        assert_eq!(
            valid_mac(" aa:bb:cc:dd:ee:ff ").as_deref(),
            Ok("AA:BB:CC:DD:EE:FF")
        );
        for bad in ["bad; address", "AA:BB:CC:DD:EE", "AA:BB:CC:DD:EE:FG", ""] {
            assert_eq!(valid_mac(bad), Err("Invalid Bluetooth address".into()));
        }
    }

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        connect_fails: bool,
    }

    impl Bus for Fake {
        async fn pair(&self, device: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("pair {device}"));
            Ok(())
        }
        async fn trust(&self, device: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("trust {device}"));
            Ok(())
        }
        async fn connect(&self, device: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("connect {device}"));
            if self.connect_fails {
                Err("asleep".into())
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn pairing_pairs_trusts_and_connects() {
        let bus = Fake {
            connect_fails: true,
            ..Default::default()
        };
        assert_eq!(pair_with(&bus, "hci0", "aa:bb:cc:dd:ee:ff").await, Ok(()));
        let dev = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF";
        assert_eq!(
            *bus.calls.lock().unwrap(),
            [
                format!("pair {dev}"),
                format!("trust {dev}"),
                format!("connect {dev}")
            ]
        );
    }

    #[tokio::test]
    async fn a_bad_address_never_reaches_bluez() {
        let bus = Fake::default();
        assert_eq!(
            pair_with(&bus, "hci0", "bad; address").await,
            Err("Invalid Bluetooth address".into())
        );
        assert!(bus.calls.lock().unwrap().is_empty());
    }
}
