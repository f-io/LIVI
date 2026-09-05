// Userspace NCM host for the iPhone network function the kernel cdc_ncm driver rejects.

use std::fs;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use nusb::transfer::{Bulk, Buffer, ControlIn, ControlType, In, Out, Recipient};
use nusb::{Device, Endpoint, MaybeFuture};

use crate::linux::{find_iphones, open_by_address}; // NcmBridge is local-USB only
use crate::ntb::{build_ntb, parse_ntb};

const NCM_CONTROL_CLASS: u8 = 0x02;
const NCM_CONTROL_SUBCLASS: u8 = 0x0d;
const NCM_DATA_CLASS: u8 = 0x0a;
const TAP_MTU_BUF: usize = 4096;
const USB_READ_BUF: usize = 32768;

static TAP_SEQ: AtomicU16 = AtomicU16::new(0);

pub struct NcmBridge {
    pub ifname: String,
    /// None when the kernel already provides the interface and nothing needs bridging.
    run: Option<Arc<AtomicBool>>,
}

impl Drop for NcmBridge {
    fn drop(&mut self) {
        if let Some(run) = &self.run {
            run.store(false, Ordering::SeqCst);
        }
    }
}

/// Control/data interface pair of the NCM function in the active configuration, plus the
/// data endpoints, all read from the descriptors so model differences don't matter.
struct NcmFunction {
    control: u8,
    data: u8,
    ep_in: u8,
    ep_out: u8,
}

fn find_ncm_function(device: &Device) -> Option<NcmFunction> {
    let config = device.active_configuration().ok()?;
    let mut control = None;
    for desc in config.interface_alt_settings() {
        if desc.class() == NCM_CONTROL_CLASS && desc.subclass() == NCM_CONTROL_SUBCLASS {
            control = Some(desc.interface_number());
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
                });
            }
        }
    }
    None
}

/// The interface the kernel's cdc_ncm driver created for this phone, if any; it carries the
/// AV link.
fn kernel_ncm_iface(sysfs: &Path) -> Option<String> {
    let root = sysfs.canonicalize().ok()?;
    for e in fs::read_dir("/sys/class/net").ok()?.flatten() {
        let Ok(driver) = e.path().join("device/driver").canonicalize() else { continue };
        if driver.file_name().and_then(|n| n.to_str()) != Some("cdc_ncm") {
            continue;
        }
        if let Ok(dev) = e.path().join("device").canonicalize()
            && dev.starts_with(&root) {
                return e.file_name().into_string().ok();
            }
    }
    None
}

fn open_tap(ifname: &str) -> Result<OwnedFd, String> {
    const TUNSETIFF: libc::c_ulong = 0x4004_54CA;
    const IFF_TAP: libc::c_short = 0x0002;
    const IFF_NO_PI: libc::c_short = 0x1000;

    let fd = unsafe { libc::open(c"/dev/net/tun".as_ptr(), libc::O_RDWR) };
    if fd < 0 {
        return Err(format!("open /dev/net/tun: {}", std::io::Error::last_os_error()));
    }
    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    for (i, b) in ifname.as_bytes().iter().take(15).enumerate() {
        ifr.ifr_name[i] = *b as libc::c_char;
    }
    ifr.ifr_ifru.ifru_flags = IFF_TAP | IFF_NO_PI;
    if unsafe { libc::ioctl(fd, TUNSETIFF, &mut ifr) } < 0 {
        let e = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(format!("TUNSETIFF {ifname}: {e}"));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn run_cmd(program: &str, args: &[&str]) {
    let _ = Command::new(program).args(args).output();
}

impl NcmBridge {
    pub fn start(serial: &str) -> Result<Self, String> {
        let dev = find_iphones()
            .into_iter()
            .find(|d| d.serial == serial)
            .ok_or_else(|| format!("iphone {serial} not found"))?;

        if let Some(ifname) = kernel_ncm_iface(&dev.sysfs) {
            println!("[ncm] using kernel cdc_ncm interface {ifname}");
            return Ok(Self { ifname, run: None });
        }

        let device = open_by_address(dev.bus, dev.address)?;
        let func = find_ncm_function(&device).ok_or("no NCM function in the active config")?;

        let _control = device
            .claim_interface(func.control)
            .wait()
            .map_err(|e| format!("claim NCM control {}: {e}", func.control))?;
        let data_iface = device
            .claim_interface(func.data)
            .wait()
            .map_err(|e| format!("claim NCM data {}: {e}", func.data))?;
        // Alt setting 1 is the one with the bulk endpoints; alt 0 carries no data.
        data_iface
            .set_alt_setting(1)
            .wait()
            .map_err(|e| format!("NCM alt setting: {e}"))?;

        let host_mac = host_mac(&device, &dev.sysfs, func.control);
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
        // One peer on this link, so duplicate address detection only costs time.
        let _ = fs::write(format!("/proc/sys/net/ipv6/conf/{ifname}/accept_dad"), "0");
        run_cmd("ip", &["link", "set", &ifname, "up"]);

        let run = Arc::new(AtomicBool::new(true));
        let tap = Arc::new(tap);

        spawn_usb_to_tap(ep_in, tap.clone(), run.clone());
        spawn_tap_to_usb(ep_out, tap, run.clone());

        println!(
            "[ncm] up on {ifname}: if{}/{} ep=0x{:02x}/0x{:02x} mac={}",
            func.control,
            func.data,
            func.ep_in,
            func.ep_out,
            host_mac.as_deref().unwrap_or("?")
        );
        Ok(Self { ifname, run: Some(run) })
    }
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
                && run.load(Ordering::SeqCst) && !is_timeout(&e) {
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

/// The host-side MAC of the link, from the USB string the CDC Ethernet functional descriptor
/// names.
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
        } else if btype == 0x24 && blen >= 4 && cur_if == control_if as i32 && raw[idx + 2] == 0x0F {
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
    let utf16: Vec<u16> = buf[2..end].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
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
