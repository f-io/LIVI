// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::time::Duration;

use nusb::transfer::{
    Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Interrupt, Out, Recipient,
};
use nusb::{Device, Endpoint, Interface, MaybeFuture};

use crate::linux::{IPhoneDev, find_iphones, open_by_address};
use crate::ntb::{build_ntb, parse_ntb};

const NCM_CONTROL_CLASS: u8 = 0x02;
const NCM_CONTROL_SUBCLASS: u8 = 0x0d;
const NCM_DATA_CLASS: u8 = 0x0a;
const TAP_MTU_BUF: usize = 4096;
const USB_READ_BUF: usize = 32768;

const GET_NTB_PARAMETERS: u8 = 0x80;
const NTB_PARAMETERS_LEN: u16 = 28;
const SET_NTB_FORMAT: u8 = 0x84;
const SET_NTB_INPUT_SIZE: u8 = 0x86;
const SET_CRC_MODE: u8 = 0x8a;
const SET_ETHERNET_PACKET_FILTER: u8 = 0x43;
const PACKET_FILTER: u16 = 0x000e;
const NTB_INPUT_SIZE: u32 = 16384;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(1);

static TAP_SEQ: AtomicU16 = AtomicU16::new(0);

pub struct NcmBridge {
    pub ifname: String,
    run: Option<Arc<AtomicBool>>,
    _control: Option<Interface>,
}

impl Drop for NcmBridge {
    fn drop(&mut self) {
        if let Some(run) = &self.run {
            run.store(false, Ordering::SeqCst);
        }
    }
}

struct NcmFunction {
    control: u8,
    data: u8,
    ep_in: u8,
    ep_out: u8,
    ep_notify: Option<u8>,
}

fn find_ncm_function(device: &Device) -> Option<NcmFunction> {
    let config = device.active_configuration().ok()?;
    let mut control = None;
    let mut ep_notify = None;
    for desc in config.interface_alt_settings() {
        if desc.class() == NCM_CONTROL_CLASS && desc.subclass() == NCM_CONTROL_SUBCLASS {
            control = Some(desc.interface_number());
            ep_notify = desc
                .endpoints()
                .find(|ep| {
                    ep.transfer_type() == nusb::descriptors::TransferType::Interrupt
                        && ep.address() & 0x80 != 0
                })
                .map(|ep| ep.address());
        }
        if let Some(control) = control.filter(|_| desc.class() == NCM_DATA_CLASS) {
            let (mut ep_in, mut ep_out) = (0u8, 0u8);
            for ep in desc.endpoints() {
                if ep.transfer_type() != nusb::descriptors::TransferType::Bulk {
                    continue;
                }
                if ep.address() & 0x80 != 0 {
                    ep_in = ep.address();
                } else {
                    ep_out = ep.address();
                }
            }
            if ep_in != 0 && ep_out != 0 {
                return Some(NcmFunction {
                    control,
                    data: desc.interface_number(),
                    ep_in,
                    ep_out,
                    ep_notify,
                });
            }
        }
    }
    None
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
        Self::bridge(&dev, &device, &func).or_else(|e| {
            eprintln!("[ncm] no bridge ({e}), the kernel's interface it is");
            let ifname = kernel_ncm_iface(&dev.sysfs).ok_or(e)?;
            println!("[ncm] using kernel cdc_ncm interface {ifname}");
            link_local_profile(&ifname);
            Ok(Self {
                ifname,
                run: None,
                _control: None,
            })
        })
    }

    fn bridge(dev: &IPhoneDev, device: &Device, func: &NcmFunction) -> Result<Self, String> {
        let control = device
            .detach_and_claim_interface(func.control)
            .wait()
            .map_err(|e| format!("claim NCM control {}: {e}", func.control))?;
        let data_iface = device
            .detach_and_claim_interface(func.data)
            .wait()
            .map_err(|e| format!("claim NCM data {}: {e}", func.data))?;
        ncm_setup(&control, func.control);
        data_iface
            .set_alt_setting(1)
            .wait()
            .map_err(|e| format!("NCM alt setting: {e}"))?;
        if let Err(e) = class_out(
            &control,
            func.control,
            SET_ETHERNET_PACKET_FILTER,
            PACKET_FILTER,
            &[],
        ) {
            eprintln!("[ncm] packet filter: {e}");
        }

        let host_mac = host_mac(device, &dev.sysfs, func.control);
        let ep_in = data_iface
            .endpoint::<Bulk, In>(func.ep_in)
            .map_err(|e| format!("NCM in endpoint: {e}"))?;
        let ep_out = data_iface
            .endpoint::<Bulk, Out>(func.ep_out)
            .map_err(|e| format!("NCM out endpoint: {e}"))?;

        let ifname = format!("cpusb{}", TAP_SEQ.fetch_add(1, Ordering::SeqCst));
        let tap = open_tap(&ifname)?;

        if let Some(mac) = &host_mac {
            run_cmd("ip", &["link", "set", &ifname, "address", mac]);
        }
        run_cmd("nmcli", &["device", "set", &ifname, "managed", "no"]);
        let _ = fs::write(format!("/proc/sys/net/ipv6/conf/{ifname}/accept_dad"), "0");
        run_cmd("ip", &["link", "set", &ifname, "up"]);

        let run = Arc::new(AtomicBool::new(true));
        let tap = Arc::new(tap);

        spawn_usb_to_tap(ep_in, tap.clone(), run.clone());
        spawn_tap_to_usb(ep_out, tap, run.clone());
        if let Some(ep) = func
            .ep_notify
            .and_then(|a| control.endpoint::<Interrupt, In>(a).ok())
        {
            spawn_notifications(ep, run.clone());
        }

        println!(
            "[ncm] up on {ifname}: if{}/{} ep=0x{:02x}/0x{:02x} mac={}",
            func.control,
            func.data,
            func.ep_in,
            func.ep_out,
            host_mac.as_deref().unwrap_or("?")
        );
        Ok(Self {
            ifname,
            run: Some(run),
            _control: Some(control),
        })
    }
}

fn class_out(
    control: &Interface,
    iface: u8,
    request: u8,
    value: u16,
    data: &[u8],
) -> Result<(), String> {
    control
        .control_out(
            ControlOut {
                control_type: ControlType::Class,
                recipient: Recipient::Interface,
                request,
                value,
                index: iface as u16,
                data,
            },
            CONTROL_TIMEOUT,
        )
        .wait()
        .map_err(|e| format!("request 0x{request:02x}: {e}"))
}

fn ncm_setup(control: &Interface, iface: u8) {
    let params = control
        .control_in(
            ControlIn {
                control_type: ControlType::Class,
                recipient: Recipient::Interface,
                request: GET_NTB_PARAMETERS,
                value: 0,
                index: iface as u16,
                length: NTB_PARAMETERS_LEN,
            },
            CONTROL_TIMEOUT,
        )
        .wait();
    if let Err(e) = params {
        eprintln!("[ncm] NTB parameters: {e}");
    }
    let steps: [(u8, &[u8]); 3] = [
        (SET_CRC_MODE, &[]),
        (SET_NTB_FORMAT, &[]),
        (SET_NTB_INPUT_SIZE, &NTB_INPUT_SIZE.to_le_bytes()),
    ];
    for (request, data) in steps {
        if let Err(e) = class_out(control, iface, request, 0, data) {
            eprintln!("[ncm] {e}");
        }
    }
}

fn spawn_notifications(mut ep: Endpoint<Interrupt, In>, run: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while run.load(Ordering::SeqCst) {
            let len = ep.max_packet_size().max(1);
            let completion = ep.transfer_blocking(Buffer::new(len), Duration::from_millis(2000));
            match completion.status {
                Ok(()) => {}
                Err(e) if is_timeout(&e) => {}
                Err(_) => return,
            }
        }
    });
}

fn spawn_usb_to_tap(mut ep_in: Endpoint<Bulk, In>, tap: Arc<OwnedFd>, run: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while run.load(Ordering::SeqCst) {
            let completion =
                ep_in.transfer_blocking(Buffer::new(USB_READ_BUF), Duration::from_millis(2000));
            if let Err(e) = completion.status {
                if run.load(Ordering::SeqCst) && !is_timeout(&e) {
                    eprintln!("[ncm] usb read ended: {e}");
                    return;
                }
                continue;
            }
            for frame in parse_ntb(&completion.buffer) {
                if write_fd(tap.as_raw_fd(), &frame).is_err() {
                    return;
                }
            }
        }
    });
}

fn spawn_tap_to_usb(mut ep_out: Endpoint<Bulk, Out>, tap: Arc<OwnedFd>, run: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut seq = 0u16;
        let mut buf = vec![0u8; TAP_MTU_BUF];
        while run.load(Ordering::SeqCst) {
            let n = match read_fd(tap.as_raw_fd(), &mut buf) {
                Ok(0) => continue,
                Ok(n) => n,
                Err(_) if !run.load(Ordering::SeqCst) => return,
                Err(_) => continue,
            };
            seq = seq.wrapping_add(1);
            let ntb = build_ntb(&buf[..n], seq);
            let mut out = Buffer::new(ntb.len());
            out.extend_from_slice(&ntb);
            let completion = ep_out.transfer_blocking(out, Duration::from_millis(3000));
            if let Err(e) = completion.status
                && run.load(Ordering::SeqCst)
                && !is_timeout(&e)
            {
                eprintln!("[ncm] usb write ended: {e}");
                return;
            }
        }
    });
}

fn is_timeout(e: &nusb::transfer::TransferError) -> bool {
    matches!(e, nusb::transfer::TransferError::Cancelled)
}

fn read_fd(fd: RawFd, buf: &mut [u8]) -> std::io::Result<usize> {
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if n < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

fn write_fd(fd: RawFd, buf: &[u8]) -> std::io::Result<()> {
    let n = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
    if n < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn host_mac(device: &Device, sysfs: &Path, control_if: u8) -> Option<String> {
    let raw = fs::read(sysfs.join("descriptors")).ok()?;
    let mut idx = 0usize;
    let mut cur_if: i32 = -1;
    let mut mac_str_idx = 0u8;
    while idx + 2 <= raw.len() {
        let blen = raw[idx] as usize;
        let btype = raw[idx + 1];
        if blen < 2 {
            break;
        }
        if btype == 0x04 && blen >= 3 {
            cur_if = raw[idx + 2] as i32;
        } else if btype == 0x24 && blen >= 4 && cur_if == control_if as i32 && raw[idx + 2] == 0x0F
        {
            mac_str_idx = raw[idx + 3];
            break;
        }
        idx += blen;
    }
    if mac_str_idx == 0 {
        return None;
    }
    let buf = device
        .control_in(
            ControlIn {
                control_type: ControlType::Standard,
                recipient: Recipient::Device,
                request: 6,
                value: (3 << 8) | mac_str_idx as u16,
                index: 0x0409,
                length: 64,
            },
            Duration::from_secs(1),
        )
        .wait()
        .ok()?;
    if buf.len() < 2 || buf[1] != 3 {
        return None;
    }
    let end = (buf[0] as usize).min(buf.len());
    let utf16: Vec<u16> = buf[2..end]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let s = String::from_utf16(&utf16).ok()?;
    if s.len() != 12 {
        return None;
    }
    Some(
        (0..12)
            .step_by(2)
            .map(|i| s[i..i + 2].to_lowercase())
            .collect::<Vec<_>>()
            .join(":"),
    )
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
