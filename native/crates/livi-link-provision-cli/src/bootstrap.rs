// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::net::Ipv6Addr;
use std::path::Path;

use crate::wire;
use livi_session_io::usb::{UsbStream, open_pipe};
use tokio::io::AsyncWriteExt;

const TYPE_OPEN: u32 = 0x01;
const TYPE_SEND_FILE: u32 = 0x99;

pub const CARRIER: &str = "/etc/profile";
pub const BOOT_HOOK: &str = "/script/livi-boot.sh";

const BOOTSTRAP: &str = include_str!("../scripts/bootstrap.sh");
const CARRIER_BODY: &str = include_str!("../scripts/profile");
const HOOK_LINE: &str = "\ncase \"$0\" in */rcS) sh /script/livi-boot.sh ;; esac\n";

fn open_payload() -> Vec<u8> {
    [800u32, 480, 30, 5, 49152, 2, 2]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

fn send_file_payload(path: &str, content: &[u8]) -> Vec<u8> {
    let mut name = path.as_bytes().to_vec();
    name.push(0);
    let mut out = Vec::with_capacity(8 + name.len() + content.len());
    out.extend_from_slice(&(name.len() as u32).to_le_bytes());
    out.extend_from_slice(&name);
    out.extend_from_slice(&(content.len() as u32).to_le_bytes());
    out.extend_from_slice(content);
    out
}

pub fn write_file(path: &str, content: &[u8]) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("runtime: {e}"))?;
    runtime.block_on(send(path, content))
}

pub fn boot_hook() -> Result<(), String> {
    write_file(BOOT_HOOK, BOOTSTRAP.as_bytes())?;
    write_file(CARRIER, format!("{CARRIER_BODY}{HOOK_LINE}").as_bytes())
}

pub fn carrier_body() -> &'static str {
    CARRIER_BODY
}

pub fn stock_dongle_once() -> bool {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return false,
    };
    runtime.block_on(async {
        match nusb::list_devices().await {
            Ok(devices) => devices
                .into_iter()
                .any(|d| livi_link_host::is_dongle(&d) && !livi_link_host::is_livi_link(&d)),
            Err(_) => false,
        }
    })
}

pub const VENDOR_AP_HOSTS: [&str; 2] = ["192.168.50.2", "192.168.43.1"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOnUsb {
    Ncm,
    NoNcm,
}

pub fn livi_link_on_usb() -> Option<LinkOnUsb> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        let devices = nusb::list_devices().await.ok()?;
        let link = devices.into_iter().find(livi_link_host::is_livi_link)?;
        let interfaces = link.interfaces().map(|i| (i.class(), i.subclass()));
        Some(if carries_ncm(interfaces) {
            LinkOnUsb::Ncm
        } else {
            LinkOnUsb::NoNcm
        })
    })
}

pub const BOOTSTRAP_MAC: [u8; 6] = [0xc2, 0x8e, 0x30, 0x53, 0x48, 0x01];

pub fn link_local(mac: [u8; 6]) -> Ipv6Addr {
    let [a, b, c, d, e, f] = mac;
    let eui = [a ^ 0x02, b, c, 0xff, 0xfe, d, e, f];
    let mut bytes = [0u8; 16];
    bytes[..2].copy_from_slice(&[0xfe, 0x80]);
    bytes[8..].copy_from_slice(&eui);
    Ipv6Addr::from(bytes)
}

pub fn link_iface() -> Option<String> {
    link_iface_in(Path::new("/sys"))
}

fn link_iface_in(sys: &Path) -> Option<String> {
    std::fs::read_dir(sys.join("class/net"))
        .ok()?
        .flatten()
        .find_map(|entry| {
            let interface = std::fs::canonicalize(entry.path().join("device")).ok()?;
            let product = std::fs::read_to_string(interface.parent()?.join("product")).ok()?;
            (product.trim() == livi_link_host::LINK_PRODUCT)
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
}

pub fn iface_up(iface: &str) -> bool {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/flags")).is_ok_and(|f| flags_up(&f))
}

fn flags_up(flags: &str) -> bool {
    u32::from_str_radix(flags.trim().trim_start_matches("0x"), 16).is_ok_and(|f| f & 1 != 0)
}

pub fn bring_up(iface: &str) -> bool {
    std::process::Command::new("ip")
        .args(["link", "set", iface, "up"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
        && iface_up(iface)
}

fn carries_ncm(interfaces: impl IntoIterator<Item = (u8, u8)>) -> bool {
    const CDC: u8 = 0x02;
    const NCM: u8 = 0x0d;
    interfaces
        .into_iter()
        .any(|(class, subclass)| class == CDC && subclass == NCM)
}

pub fn scan() -> Vec<(u16, u16, String)> {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    runtime.block_on(async {
        match nusb::list_devices().await {
            Ok(devices) => devices
                .into_iter()
                .map(|d| {
                    (
                        d.vendor_id(),
                        d.product_id(),
                        d.product_string().unwrap_or("").to_string(),
                    )
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    })
}

async fn send(path: &str, content: &[u8]) -> Result<(), String> {
    let mut info = None;
    for _ in 0..20 {
        if let Ok(devices) = nusb::list_devices().await {
            info = devices
                .into_iter()
                .find(|d| livi_link_host::is_dongle(d) && !livi_link_host::is_livi_link(d));
        }
        if info.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let info = info.ok_or("no dongle on the bus, unplug it and plug it back in")?;

    let dev = info.open().await.map_err(|e| format!("open dongle: {e}"))?;
    let pipe = open_pipe(&dev)
        .await
        .map_err(|e| format!("{e}, unplug the dongle and plug it back in"))?;
    let mut stream = UsbStream::new(pipe);

    stream
        .write_all(&wire::message(TYPE_OPEN, &open_payload()))
        .await
        .map_err(|e| format!("open handshake: {e}"))?;
    stream
        .write_all(&wire::message(
            TYPE_SEND_FILE,
            &send_file_payload(path, content),
        ))
        .await
        .map_err(|e| format!("send {path}: {e}"))?;
    stream.flush().await.map_err(|e| format!("flush: {e}"))?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_cdc_ncm_interface_counts_as_ncm() {
        assert!(carries_ncm([(0x02, 0x0d), (0x0a, 0x00)]));
        assert!(!carries_ncm([(0x02, 0x06), (0x0a, 0x00)]));
        assert!(!carries_ncm([(0xff, 0x0d)]));
        assert!(!carries_ncm([]));
    }

    #[test]
    fn a_send_file_payload_carries_the_path_and_the_bytes() {
        let p = send_file_payload("/script/custom_init.sh", b"hi");
        assert_eq!(u32::from_le_bytes(p[..4].try_into().unwrap()), 23);
        assert_eq!(&p[4..26], b"/script/custom_init.sh");
        assert_eq!(p[26], 0);
        assert_eq!(u32::from_le_bytes(p[27..31].try_into().unwrap()), 2);
        assert_eq!(&p[31..], b"hi");
    }

    #[test]
    fn the_open_payload_is_the_geometry_the_dongle_expects() {
        let p = open_payload();
        assert_eq!(p.len(), 28);
        assert_eq!(u32::from_le_bytes(p[..4].try_into().unwrap()), 800);
        assert_eq!(u32::from_le_bytes(p[24..].try_into().unwrap()), 2);
    }

    #[test]
    fn the_link_local_address_is_the_one_the_bootstrap_mac_gives() {
        assert_eq!(
            link_local(BOOTSTRAP_MAC).to_string(),
            "fe80::c08e:30ff:fe53:4801"
        );
        let mac = BOOTSTRAP_MAC.map(|b| format!("{b:02x}")).join(":");
        assert!(BOOTSTRAP.contains(&format!("ifconfig ncm0 hw ether {mac}")));
    }

    #[test]
    fn only_the_up_flag_counts() {
        assert!(flags_up("0x1003\n"));
        assert!(!flags_up("0x1002\n"));
        assert!(!flags_up("garbage"));
    }

    #[cfg(unix)]
    #[test]
    fn the_interface_is_found_by_the_usb_product_behind_it() {
        let sys = std::env::temp_dir().join(format!("livi-sys-{}", std::process::id()));
        let usb = |name: &str, product: &str| {
            let device = sys.join("devices").join(name);
            std::fs::create_dir_all(device.join(format!("{name}:1.0"))).unwrap();
            std::fs::write(device.join("product"), format!("{product}\n")).unwrap();
            device.join(format!("{name}:1.0"))
        };
        let net = |iface: &str, interface: &Path| {
            std::fs::create_dir_all(sys.join("class/net").join(iface)).unwrap();
            std::os::unix::fs::symlink(interface, sys.join("class/net").join(iface).join("device"))
                .unwrap();
        };
        net("eno1", &usb("1-2", "Some Adapter"));
        assert_eq!(link_iface_in(&sys), None);
        net("wwx2698d934fda1", &usb("1-4", "LIVI Link"));
        assert_eq!(link_iface_in(&sys).as_deref(), Some("wwx2698d934fda1"));
        std::fs::remove_dir_all(&sys).unwrap();
    }

    #[test]
    fn the_bootstrap_announces_the_product_our_system_announces() {
        let ours = include_str!("../../../../scripts/livi-link/common/initramfs/net-up");
        assert!(BOOTSTRAP.contains("printf 'LIVI Link' > \"$A/iProduct\""));
        assert!(ours.contains("echo \"LIVI Link\" > $G/strings/0x409/product"));
    }
}
