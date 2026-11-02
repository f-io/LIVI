// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "android")]
pub use android::{OpenTun, TunRequest};
#[cfg(target_os = "linux")]
pub use linux::drop_name_bound_profiles;

use std::io;
use std::net::Ipv6Addr;
use std::num::NonZeroU8;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use nusb::descriptors::TransferType;
use nusb::descriptors::language_id::US_ENGLISH;
use nusb::transfer::{
    Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Interrupt, Out, Recipient, TransferError,
};
use nusb::{Device, Endpoint, Interface, MaybeFuture};

use crate::Mac;
use crate::ntb::{build_ntb, parse_ntb};

const NCM_CONTROL_CLASS: u8 = 0x02;
const NCM_CONTROL_SUBCLASS: u8 = 0x0d;
const NCM_DATA_CLASS: u8 = 0x0a;
const CS_INTERFACE: u8 = 0x24;
const ETHERNET_FUNCTIONAL: u8 = 0x0f;
const FRAME_BUF: usize = 4096;
const USB_READ_BUF: usize = 32768;
const POLL: Duration = Duration::from_millis(500);
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);

const GET_NTB_PARAMETERS: u8 = 0x80;
const NTB_PARAMETERS_LEN: u16 = 28;
const SET_NTB_FORMAT: u8 = 0x84;
const SET_NTB_INPUT_SIZE: u8 = 0x86;
const SET_CRC_MODE: u8 = 0x8a;
const SET_ETHERNET_PACKET_FILTER: u8 = 0x43;
const PACKET_FILTER: u16 = 0x000e;
const NTB_INPUT_SIZE: u32 = 16384;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(1);

trait Link: Send + Sync + 'static {
    fn deliver(&self, frame: &[u8]) -> io::Result<Option<Vec<u8>>>;
    fn next(&self) -> io::Result<Option<Vec<u8>>>;
}

pub struct NcmBridge {
    pub ifname: String,
    pub address: Option<Ipv6Addr>,
    run: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    _control: Option<Interface>,
}

impl NcmBridge {
    fn up(ifname: String, usb: Usb, link: impl Link) -> Self {
        let run = Arc::new(AtomicBool::new(true));
        let link = Arc::new(link);
        let out = Arc::new(Sender {
            ep: Mutex::new(usb.ep_out),
            seq: AtomicU16::new(0),
        });
        let mut threads = vec![
            spawn_from_phone(usb.ep_in, link.clone(), out.clone(), run.clone()),
            spawn_to_phone(link, out, run.clone()),
        ];
        if let Some(ep) = usb.ep_notify {
            threads.push(spawn_notifications(ep, run.clone()));
        }
        Self {
            ifname,
            address: None,
            run,
            threads,
            _control: Some(usb.control),
        }
    }

    #[cfg(target_os = "linux")]
    fn system(ifname: String) -> Self {
        Self {
            ifname,
            address: None,
            run: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
            _control: None,
        }
    }
}

impl Drop for NcmBridge {
    fn drop(&mut self) {
        self.run.store(false, Ordering::SeqCst);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

struct NcmFunction {
    control: u8,
    data: u8,
    ep_in: u8,
    ep_out: u8,
    ep_notify: Option<u8>,
    mac_string: Option<NonZeroU8>,
}

fn find_ncm_function(device: &Device) -> Option<NcmFunction> {
    let config = device.active_configuration().ok()?;
    let mut control = None;
    let mut ep_notify = None;
    let mut mac_string = None;
    for desc in config.interface_alt_settings() {
        if desc.class() == NCM_CONTROL_CLASS && desc.subclass() == NCM_CONTROL_SUBCLASS {
            control = Some(desc.interface_number());
            ep_notify = desc
                .endpoints()
                .find(|ep| {
                    ep.transfer_type() == TransferType::Interrupt && ep.address() & 0x80 != 0
                })
                .map(|ep| ep.address());
            mac_string = desc
                .descriptors()
                .find(|d| {
                    d.descriptor_type() == CS_INTERFACE && d.get(2) == Some(&ETHERNET_FUNCTIONAL)
                })
                .and_then(|d| d.get(3).copied())
                .and_then(NonZeroU8::new);
        }
        if let Some(control) = control.filter(|_| desc.class() == NCM_DATA_CLASS) {
            let (mut ep_in, mut ep_out) = (0u8, 0u8);
            for ep in desc.endpoints() {
                if ep.transfer_type() != TransferType::Bulk {
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
                    mac_string,
                });
            }
        }
    }
    None
}

fn host_mac(device: &Device, func: &NcmFunction) -> Option<Mac> {
    let text = device
        .get_string_descriptor(func.mac_string?, US_ENGLISH, CONTROL_TIMEOUT)
        .wait()
        .ok()?;
    parse_mac(text.trim_end_matches('\0'))
}

fn parse_mac(text: &str) -> Option<Mac> {
    if text.len() != 12 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(mac)
}

fn format_mac(mac: &Mac) -> String {
    mac.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

struct Usb {
    control: Interface,
    ep_in: Endpoint<Bulk, In>,
    ep_out: Endpoint<Bulk, Out>,
    ep_notify: Option<Endpoint<Interrupt, In>>,
}

fn claim(device: &Device, number: u8) -> Result<Interface, String> {
    device
        .detach_and_claim_interface(number)
        .wait()
        .or_else(|_| device.claim_interface(number).wait())
        .map_err(|e| format!("claim NCM interface {number}: {e}"))
}

fn open_usb(device: &Device, func: &NcmFunction) -> Result<Usb, String> {
    let control = claim(device, func.control)?;
    let data = claim(device, func.data)?;
    ncm_setup(&control, func.control);
    data.set_alt_setting(1)
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
    let ep_in = data
        .endpoint::<Bulk, In>(func.ep_in)
        .map_err(|e| format!("NCM in endpoint: {e}"))?;
    let ep_out = data
        .endpoint::<Bulk, Out>(func.ep_out)
        .map_err(|e| format!("NCM out endpoint: {e}"))?;
    let ep_notify = func
        .ep_notify
        .and_then(|a| control.endpoint::<Interrupt, In>(a).ok());
    Ok(Usb {
        control,
        ep_in,
        ep_out,
        ep_notify,
    })
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

struct Sender {
    ep: Mutex<Endpoint<Bulk, Out>>,
    seq: AtomicU16,
}

impl Sender {
    fn send(&self, frame: &[u8]) -> Result<(), TransferError> {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let ntb = build_ntb(frame, seq);
        let mut out = Buffer::new(ntb.len());
        out.extend_from_slice(&ntb);
        self.ep
            .lock()
            .unwrap()
            .transfer_blocking(out, WRITE_TIMEOUT)
            .status
    }
}

fn spawn_notifications(mut ep: Endpoint<Interrupt, In>, run: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while run.load(Ordering::SeqCst) {
            let len = ep.max_packet_size().max(1);
            match ep.transfer_blocking(Buffer::new(len), POLL).status {
                Ok(()) | Err(TransferError::Cancelled) => {}
                Err(_) => return,
            }
        }
    })
}

fn spawn_from_phone(
    mut ep_in: Endpoint<Bulk, In>,
    link: Arc<impl Link>,
    out: Arc<Sender>,
    run: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while run.load(Ordering::SeqCst) {
            let completion = ep_in.transfer_blocking(Buffer::new(USB_READ_BUF), POLL);
            match completion.status {
                Ok(()) => {}
                Err(TransferError::Cancelled) => continue,
                Err(e) => {
                    if run.load(Ordering::SeqCst) {
                        eprintln!("[ncm] usb read ended: {e}");
                    }
                    return;
                }
            }
            for frame in parse_ntb(&completion.buffer) {
                match link.deliver(&frame) {
                    Ok(None) => {}
                    Ok(Some(answer)) => {
                        let _ = out.send(&answer);
                    }
                    Err(e) => {
                        eprintln!("[ncm] local write ended: {e}");
                        return;
                    }
                }
            }
        }
    })
}

fn spawn_to_phone(link: Arc<impl Link>, out: Arc<Sender>, run: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while run.load(Ordering::SeqCst) {
            let frame = match link.next() {
                Ok(Some(frame)) => frame,
                Ok(None) => continue,
                Err(e) => {
                    eprintln!("[ncm] local read ended: {e}");
                    return;
                }
            };
            match out.send(&frame) {
                Ok(()) | Err(TransferError::Cancelled) => {}
                Err(e) => {
                    if run.load(Ordering::SeqCst) {
                        eprintln!("[ncm] usb write ended: {e}");
                    }
                    return;
                }
            }
        }
    })
}

fn read_within(fd: &OwnedFd, buf: &mut [u8]) -> io::Result<Option<usize>> {
    let mut pfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut pfd, 1, POLL.as_millis() as libc::c_int) };
    if ready < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::Interrupted {
            Ok(None)
        } else {
            Err(e)
        };
    }
    if ready == 0 {
        return Ok(None);
    }
    let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
    if n < 0 {
        let e = io::Error::last_os_error();
        return match e.kind() {
            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock => Ok(None),
            _ => Err(e),
        };
    }
    Ok(Some(n as usize))
}

fn write_all(fd: &OwnedFd, buf: &[u8]) -> io::Result<()> {
    let n = unsafe { libc::write(fd.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mac_string_reads_as_six_bytes() {
        assert_eq!(
            parse_mac("0E6AC4630DD2"),
            Some([0x0e, 0x6a, 0xc4, 0x63, 0x0d, 0xd2])
        );
        assert_eq!(parse_mac("0E6AC4630DD"), None);
        assert_eq!(parse_mac("0E6AC4630DZ2"), None);
        assert_eq!(
            format_mac(&[0x0e, 0x6a, 0xc4, 0x63, 0x0d, 0xd2]),
            "0e:6a:c4:63:0d:d2"
        );
    }
}
