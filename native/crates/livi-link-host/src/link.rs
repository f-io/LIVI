// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const LINK_NAME: &str = "livi-link.local";

pub const CHOICE: &str = "livi-link";

static WATCHED: AtomicBool = AtomicBool::new(false);
static ON_BUS: AtomicBool = AtomicBool::new(false);

pub(crate) fn watching() {
    WATCHED.store(true, Ordering::SeqCst);
}

pub(crate) fn set_on_bus(on: bool) {
    ON_BUS.store(on, Ordering::SeqCst);
}

pub fn absent() -> bool {
    WATCHED.load(Ordering::SeqCst) && !ON_BUS.load(Ordering::SeqCst)
}

pub fn connect(port: u16, timeout: Duration) -> io::Result<TcpStream> {
    if absent() {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "the LIVI Link is not on the bus",
        ));
    }
    livi_net::connect((LINK_NAME, port), timeout)
}

pub fn resolves() -> bool {
    if absent() {
        return false;
    }
    (LINK_NAME, 0u16)
        .to_socket_addrs()
        .map(|mut a| a.any(|a| a.is_ipv4()))
        .unwrap_or(false)
}

pub fn addr(port: u16) -> String {
    format!("{LINK_NAME}:{port}")
}

#[cfg(target_os = "linux")]
pub fn host_iface() -> Option<String> {
    host_iface_in(std::path::Path::new("/sys/class/net"))
}

#[cfg(target_os = "linux")]
pub fn attached() -> bool {
    host_iface().is_some()
}

#[cfg(not(target_os = "linux"))]
pub fn attached() -> bool {
    own_ipv4().into_iter().any(on_link)
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn on_link(ip: std::net::Ipv4Addr) -> bool {
    matches!(ip.octets(), [10, 10, 10, _])
}

#[cfg(not(target_os = "linux"))]
fn own_ipv4() -> Vec<std::net::Ipv4Addr> {
    let mut out = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills the list or fails, every entry is read before freeifaddrs.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return out;
        }
        let mut cur = list;
        while !cur.is_null() {
            let addr = (*cur).ifa_addr;
            if !addr.is_null() && i32::from((*addr).sa_family) == libc::AF_INET {
                let sin = &*(addr as *const libc::sockaddr_in);
                out.push(std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
            }
            cur = (*cur).ifa_next;
        }
        libc::freeifaddrs(list);
    }
    out
}

#[cfg(target_os = "linux")]
const PROFILE: &str = "livi-link";

#[cfg(target_os = "linux")]
fn profile_args(exists: bool, iface: &str) -> Vec<&str> {
    let head: &[&str] = if exists {
        &["connection", "modify", PROFILE]
    } else {
        &["connection", "add", "type", "ethernet", "con-name", PROFILE]
    };
    let settings = [
        "connection.interface-name",
        iface,
        "ipv4.method",
        "auto",
        "ipv6.method",
        "link-local",
        "connection.autoconnect",
        "yes",
        "connection.autoconnect-priority",
        "999",
    ];
    head.iter().copied().chain(settings).collect()
}

#[cfg(target_os = "linux")]
pub fn take_address(iface: &str) -> bool {
    let nmcli = |args: &[&str]| {
        std::process::Command::new("nmcli")
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let exists = nmcli(&["-g", "connection.id", "connection", "show", PROFILE]);
    nmcli(&profile_args(exists, iface)) && nmcli(&["connection", "up", PROFILE, "ifname", iface])
}

#[cfg(target_os = "linux")]
fn host_iface_in(net: &std::path::Path) -> Option<String> {
    std::fs::read_dir(net).ok()?.flatten().find_map(|entry| {
        let product = std::fs::read_to_string(entry.path().join("device/../product")).ok()?;
        (product.trim() == crate::LINK_PRODUCT).then(|| entry.file_name().into_string().ok())?
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn a_watched_bus_without_the_link_answers_at_once() {
        watching();
        set_on_bus(false);
        let started = std::time::Instant::now();
        let refused = connect(livi_net::port::CONTROL, Duration::from_secs(3));
        assert_eq!(refused.unwrap_err().kind(), io::ErrorKind::NotConnected);
        assert!(!resolves());
        assert!(started.elapsed() < Duration::from_millis(100));
        set_on_bus(true);
        assert!(!absent());
    }

    #[test]
    fn the_link_is_the_interface_on_the_gadget() {
        let root = std::env::temp_dir().join(format!("livi-link-iface-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let gadget = |name: &str, product: &str| {
            let usb = root.join("devices").join(name);
            std::fs::create_dir_all(usb.join("iface")).unwrap();
            std::fs::write(usb.join("product"), format!("{product}\n")).unwrap();
            usb.join("iface")
        };
        let net = root.join("net");
        for (iface, device) in [
            ("wlp102s0", gadget("1-1", "Wireless Card")),
            ("usb0", gadget("1-2", crate::LINK_PRODUCT)),
        ] {
            std::fs::create_dir_all(net.join(iface)).unwrap();
            symlink(device, net.join(iface).join("device")).unwrap();
        }
        std::fs::create_dir_all(net.join("lo")).unwrap();

        assert_eq!(host_iface_in(&net).as_deref(), Some("usb0"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_profile_follows_the_links_interface_and_asks_for_an_address() {
        let added = profile_args(false, "usb0");
        assert_eq!(
            &added[..6],
            [
                "connection",
                "add",
                "type",
                "ethernet",
                "con-name",
                "livi-link"
            ]
        );
        let modified = profile_args(true, "usb1");
        assert_eq!(&modified[..3], ["connection", "modify", "livi-link"]);
        for args in [&added, &modified] {
            let pairs: Vec<_> = args.windows(2).collect();
            assert!(pairs.contains(&["ipv4.method", "auto"].as_slice()));
            assert!(pairs.contains(&["ipv6.method", "link-local"].as_slice()));
        }
        assert!(
            modified
                .windows(2)
                .any(|w| w == ["connection.interface-name", "usb1"])
        );
    }
}

#[cfg(test)]
mod subnet_tests {
    use super::on_link;

    #[test]
    fn only_the_links_subnet_counts() {
        assert!(on_link("10.10.10.100".parse().unwrap()));
        assert!(!on_link("10.10.11.100".parse().unwrap()));
        assert!(!on_link("192.168.1.10".parse().unwrap()));
    }
}
