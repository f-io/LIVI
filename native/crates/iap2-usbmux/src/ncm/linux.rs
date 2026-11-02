// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU16, Ordering};

use nusb::Device;

use super::{
    FRAME_BUF, Link, NcmBridge, NcmFunction, find_ncm_function, format_mac, host_mac, open_usb,
    read_within, write_all,
};
use crate::linux::{find_iphones, open_by_address};

static TAP_SEQ: AtomicU16 = AtomicU16::new(0);

struct Tap(OwnedFd);

impl Link for Tap {
    fn deliver(&self, frame: &[u8]) -> io::Result<Option<Vec<u8>>> {
        write_all(&self.0, frame).map(|()| None)
    }

    fn next(&self) -> io::Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; FRAME_BUF];
        Ok(read_within(&self.0, &mut buf)?.filter(|n| *n > 0).map(|n| {
            buf.truncate(n);
            buf
        }))
    }
}

fn kernel_ncm_iface(sysfs: &Path) -> Option<String> {
    let root = sysfs.canonicalize().ok()?;
    for e in fs::read_dir("/sys/class/net").ok()?.flatten() {
        let Ok(driver) = e.path().join("device/driver").canonicalize() else {
            continue;
        };
        if driver.file_name().and_then(|n| n.to_str()) != Some("cdc_ncm") {
            continue;
        }
        if let Ok(dev) = e.path().join("device").canonicalize()
            && dev.starts_with(&root)
        {
            return e.file_name().into_string().ok();
        }
    }
    None
}

fn open_tap(ifname: &str) -> Result<OwnedFd, String> {
    const TUNSETIFF: libc::c_ulong = 0x4004_54CA;
    const IFF_TAP: libc::c_short = 0x0002;
    const IFF_NO_PI: libc::c_short = 0x1000;

    let fd = unsafe { libc::open(c"/dev/net/tun".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(format!(
            "open /dev/net/tun: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    for (i, b) in ifname.as_bytes().iter().take(15).enumerate() {
        ifr.ifr_name[i] = *b as libc::c_char;
    }
    ifr.ifr_ifru.ifru_flags = IFF_TAP | IFF_NO_PI;
    if unsafe { libc::ioctl(fd, TUNSETIFF as _, &mut ifr) } < 0 {
        let e = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(format!("TUNSETIFF {ifname}: {e}"));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn run_cmd(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

const PROFILE_PREFIX: &str = "livi-carkit-";

fn bound_to_a_name(profile: &str) -> bool {
    profile
        .strip_prefix(PROFILE_PREFIX)
        .is_some_and(|rest| rest.len() != 12 || !rest.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn drop_name_bound_profiles() {
    let Ok(out) = Command::new("nmcli")
        .args(["-g", "NAME", "connection", "show"])
        .output()
    else {
        return;
    };
    for name in String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|n| bound_to_a_name(n))
    {
        let gone = run_cmd("nmcli", &["connection", "delete", name]);
        println!(
            "[ncm] NetworkManager profile {name} bound to an interface name: {}",
            if gone { "deleted" } else { "not deleted" }
        );
    }
}

fn link_local_profile(ifname: &str) {
    let _ = fs::write(format!("/proc/sys/net/ipv6/conf/{ifname}/accept_dad"), "0");
    run_cmd("ip", &["link", "set", ifname, "up"]);
    let Ok(mac) = fs::read_to_string(format!("/sys/class/net/{ifname}/address")) else {
        return;
    };
    let mac = mac.trim().to_uppercase();
    let name = format!("{PROFILE_PREFIX}{}", mac.replace(':', ""));
    if !run_cmd(
        "nmcli",
        &["-g", "connection.id", "connection", "show", &name],
    ) {
        let added = run_cmd(
            "nmcli",
            &[
                "connection",
                "add",
                "type",
                "ethernet",
                "con-name",
                &name,
                "ethernet.mac-address",
                &mac,
                "ipv4.method",
                "disabled",
                "ipv6.method",
                "link-local",
                "ipv6.addr-gen-mode",
                "eui64",
                "connection.autoconnect",
                "yes",
                "connection.autoconnect-priority",
                "999",
            ],
        );
        println!(
            "[ncm] NetworkManager profile {name}: {}",
            if added { "added" } else { "not added" }
        );
    }
    run_cmd("nmcli", &["connection", "up", &name, "ifname", ifname]);
}

impl NcmBridge {
    pub fn start(serial: &str) -> Result<Self, String> {
        let dev = find_iphones()
            .into_iter()
            .find(|d| d.serial == serial)
            .ok_or_else(|| format!("iphone {serial} not found"))?;

        let device = open_by_address(dev.bus, dev.address)?;
        let func = find_ncm_function(&device).ok_or("no NCM function in the active config")?;
        Self::bridge(&device, &func).or_else(|e| {
            eprintln!("[ncm] no bridge ({e}), the kernel's interface it is");
            let ifname = kernel_ncm_iface(&dev.sysfs).ok_or(e)?;
            println!("[ncm] using kernel cdc_ncm interface {ifname}");
            link_local_profile(&ifname);
            Ok(Self::system(ifname))
        })
    }

    fn bridge(device: &Device, func: &NcmFunction) -> Result<Self, String> {
        let usb = open_usb(device, func)?;
        let host_mac = host_mac(device, func).map(|mac| format_mac(&mac));

        let ifname = format!("cpusb{}", TAP_SEQ.fetch_add(1, Ordering::SeqCst));
        let tap = open_tap(&ifname)?;

        if let Some(mac) = &host_mac {
            run_cmd("ip", &["link", "set", &ifname, "address", mac]);
        }
        run_cmd("nmcli", &["device", "set", &ifname, "managed", "no"]);
        let _ = fs::write(format!("/proc/sys/net/ipv6/conf/{ifname}/accept_dad"), "0");
        run_cmd("ip", &["link", "set", &ifname, "up"]);

        println!(
            "[ncm] up on {ifname}: if{}/{} ep=0x{:02x}/0x{:02x} mac={}",
            func.control,
            func.data,
            func.ep_in,
            func.ep_out,
            host_mac.as_deref().unwrap_or("?")
        );
        Ok(Self::up(ifname, usb, Tap(tap)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_profiles_named_after_an_interface_go() {
        assert!(bound_to_a_name("livi-carkit-usb0"));
        assert!(bound_to_a_name("livi-carkit-enx0e6ac4630dd2"));
        assert!(!bound_to_a_name("livi-carkit-0E6AC4630DD2"));
        assert!(!bound_to_a_name("Wired connection 2"));
    }
}
