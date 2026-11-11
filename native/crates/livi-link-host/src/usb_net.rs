// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iap2_usbmux::{FromPhone, Ip, Link, NcmBridge, Port, Tunnel};
use nusb::MaybeFuture;

use crate::net4::{Incoming, Mac, Net4, Outgoing};

const IFNAME: &str = "livi-link";
const ETHERTYPE_IPV6: [u8; 2] = [0x86, 0xdd];
const ASK_EVERY: Duration = Duration::from_secs(2);
const IDLE: Duration = Duration::from_millis(200);

#[derive(Debug, thiserror::Error)]
pub enum AttachError {
    #[error("list devices: {0}")]
    List(nusb::Error),
    #[error("no LIVI Link on the bus")]
    NoLink,
    #[error("open the LIVI Link: {0}")]
    Open(nusb::Error),
    #[error("{0}")]
    Bridge(String),
}

pub struct UsbNet {
    _bridge: NcmBridge,
}

pub fn attach(tunnel: Arc<Tunnel>) -> Result<UsbNet, AttachError> {
    let info = nusb::list_devices()
        .wait()
        .map_err(AttachError::List)?
        .find(crate::is_livi_link)
        .ok_or(AttachError::NoLink)?;
    let serial = info.serial_number().unwrap_or("").to_owned();
    let device = info.open().wait().map_err(AttachError::Open)?;
    let bridge = NcmBridge::over(&device, &serial, IFNAME, |mac| {
        let owner = tunnel.clone();
        Net {
            mac,
            net: Mutex::new(Net4::new(mac, xid())),
            ip: Ip::shared(mac, tunnel.address(), Box::new(move |a| owner.owns(a))),
            tunnel,
            port: Mutex::new(None),
            asked: Mutex::new(None),
        }
    })
    .map_err(|e| AttachError::Bridge(e.to_string()))?;
    println!("[link] the LIVI Link {serial} is on USB, asking for an address");
    Ok(UsbNet { _bridge: bridge })
}

fn xid() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
        .unwrap_or(0x4c49_5649)
}

struct Net {
    mac: Mac,
    net: Mutex<Net4>,
    ip: Ip,
    tunnel: Arc<Tunnel>,
    port: Mutex<Option<Arc<Port>>>,
    asked: Mutex<Option<Instant>>,
}

impl Net {
    fn port(&self) -> Option<Arc<Port>> {
        self.port.lock().unwrap().clone()
    }

    fn due(&self) -> Option<Vec<u8>> {
        let net = self.net.lock().unwrap();
        let every = match net.lease() {
            Some(lease) if self.port().is_some() => {
                Duration::from_secs(u64::from(lease.seconds.max(2) / 2))
            }
            _ => ASK_EVERY,
        };
        let mut asked = self.asked.lock().unwrap();
        if asked.is_some_and(|at| at.elapsed() < every) {
            return None;
        }
        *asked = Some(Instant::now());
        Some(net.ask())
    }

    fn leased(&self, lease: crate::net4::Lease) {
        let v4 = Some((lease.address, lease.prefix));
        let mut port = self.port.lock().unwrap();
        let joined = match port.as_ref() {
            Some(port) => port.set_v4(v4),
            None => self.tunnel.join(v4).map(|p| port.replace(p).is_none()),
        };
        match joined {
            Ok(false) => crate::link::served_by(Some(lease.server)),
            Ok(true) => {
                crate::link::served_by(Some(lease.server));
                println!(
                    "[link] {}/{} from {} and {} on the LIVI Link",
                    lease.address,
                    lease.prefix,
                    lease.server,
                    self.tunnel.address()
                );
            }
            Err(e) => {
                eprintln!("[link] no interface for the LIVI Link: {e}");
                crate::link::served_by(None);
                *port = None;
                *self.net.lock().unwrap() = Net4::new(self.mac, xid());
            }
        }
    }
}

impl Link for Net {
    fn deliver(&self, frame: &[u8]) -> io::Result<Option<Vec<u8>>> {
        if frame.get(12..14) == Some(&ETHERTYPE_IPV6) {
            let Some(port) = self.port() else {
                return Ok(None);
            };
            return match self.ip.incoming(frame) {
                FromPhone::Deliver(packet) => port.write(packet).map(|()| None),
                FromPhone::Answer(answer) => Ok(Some(answer)),
                FromPhone::Ignore => Ok(None),
            };
        }
        let incoming = self.net.lock().unwrap().incoming(frame);
        let lease = match incoming {
            Incoming::Deliver(packet) => {
                if let Some(port) = self.port() {
                    port.write(packet)?;
                }
                return Ok(None);
            }
            Incoming::Answer(answer) => return Ok(Some(answer)),
            Incoming::Leased(lease) => lease,
            Incoming::Ignore => return Ok(None),
        };
        self.leased(lease);
        Ok(None)
    }

    fn next(&self) -> io::Result<Option<Vec<u8>>> {
        if let Some(ask) = self.due() {
            return Ok(Some(ask));
        }
        let Some(port) = self.port() else {
            std::thread::sleep(IDLE);
            return Ok(None);
        };
        let Some(packet) = port.recv() else {
            return Ok(None);
        };
        Ok(match packet.first().map(|b| b >> 4) {
            Some(6) => self.ip.outgoing(&packet),
            _ => match self.net.lock().unwrap().outgoing(&packet) {
                Outgoing::Send(frame) | Outgoing::Resolve(frame) => Some(frame),
                Outgoing::Drop => None,
            },
        })
    }
}
