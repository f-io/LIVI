// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashSet;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::OwnedFd;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{FRAME_BUF, POLL, read_within, write_all};
use crate::ether::link_local;
use crate::{Error, Mac};

const LINK_LOCAL_PREFIX: u8 = 64;
const MTU: u16 = 1500;
const QUEUE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunRequest {
    pub addresses: Vec<(IpAddr, u8)>,
    pub mtu: u16,
}

pub type OpenTun = Arc<dyn Fn(&TunRequest) -> io::Result<OwnedFd> + Send + Sync>;

// Android gives an app one VPN interface, and opening another closes the first: the iPhone's NCM
// link and the LIVI Link share it as ports, behind one link-local address of their own.
pub struct Tunnel {
    open: OpenTun,
    address: Ipv6Addr,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    fd: Option<Arc<OwnedFd>>,
    ifname: Option<String>,
    owned: Vec<Ipv6Addr>,
    ports: Vec<Weak<Port>>,
    reading: bool,
}

pub struct Port {
    tunnel: Arc<Tunnel>,
    v4: Mutex<Option<(Ipv4Addr, u8)>>,
    neighbours: Mutex<HashSet<IpAddr>>,
    tx: SyncSender<Vec<u8>>,
    rx: Mutex<Receiver<Vec<u8>>>,
}

impl Tunnel {
    pub fn new(open: OpenTun) -> Arc<Self> {
        Arc::new(Self {
            open,
            address: link_local(made_up_mac()),
            inner: Mutex::new(Inner::default()),
        })
    }

    pub fn address(&self) -> Ipv6Addr {
        self.address
    }

    pub fn ifname(&self) -> Option<String> {
        self.inner().ifname.clone()
    }

    pub fn owns(&self, address: Ipv6Addr) -> bool {
        if address == self.address {
            return true;
        }
        let mut inner = self.inner();
        if inner.owned.contains(&address) {
            return true;
        }
        inner.owned = inner
            .ifname
            .as_deref()
            .map(addresses_of)
            .unwrap_or_default();
        inner.owned.contains(&address)
    }

    pub fn join(self: &Arc<Self>, v4: Option<(Ipv4Addr, u8)>) -> Result<Arc<Port>, Error> {
        let (tx, rx) = sync_channel(QUEUE);
        let port = Arc::new(Port {
            tunnel: self.clone(),
            v4: Mutex::new(v4),
            neighbours: Mutex::new(HashSet::new()),
            tx,
            rx: Mutex::new(rx),
        });
        let mut inner = self.inner();
        inner.ports.retain(|p| p.strong_count() > 0);
        inner.ports.push(Arc::downgrade(&port));
        if (inner.fd.is_none() || v4.is_some())
            && let Err(e) = self.reopen(&mut inner)
        {
            drop(inner);
            *port.v4.lock().unwrap() = None;
            return Err(e);
        }
        if !inner.reading {
            inner.reading = true;
            let tunnel = Arc::downgrade(self);
            std::thread::spawn(move || read(tunnel));
        }
        Ok(port)
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn ports(&self) -> Vec<Arc<Port>> {
        self.inner()
            .ports
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }

    fn reopen(&self, inner: &mut Inner) -> Result<(), Error> {
        let mut addresses = vec![(IpAddr::V6(self.address), LINK_LOCAL_PREFIX)];
        for port in inner.ports.iter().filter_map(Weak::upgrade) {
            if let Some((address, prefix)) = *port.v4.lock().unwrap() {
                addresses.push((IpAddr::V4(address), prefix));
            }
        }
        let request = TunRequest {
            addresses,
            mtu: MTU,
        };
        let fd = (self.open)(&request).map_err(Error::OpenTun)?;
        let ifname =
            super::android::iface_with(self.address).ok_or(Error::NoInterface(self.address))?;
        if inner.ifname.as_ref().is_some_and(|old| *old != ifname) {
            println!("[tun] the interface moved to {ifname}");
        }
        inner.fd = Some(Arc::new(fd));
        inner.owned = addresses_of(&ifname);
        inner.ifname = Some(ifname);
        Ok(())
    }

    fn left(&self, v4: bool) {
        let mut inner = self.inner();
        inner.ports.retain(|p| p.strong_count() > 0);
        if inner.ports.is_empty() {
            inner.fd = None;
            inner.ifname = None;
            inner.owned.clear();
        } else if v4 && let Err(e) = self.reopen(&mut inner) {
            eprintln!("[tun] {e}");
        }
    }

    fn fd(&self) -> Option<Arc<OwnedFd>> {
        self.inner().fd.clone()
    }

    fn dispatch(&self, packet: &[u8]) {
        let ports = self.ports();
        let Some(dest) = destination(packet) else {
            return;
        };
        let mut to: Vec<&Arc<Port>> = Vec::new();
        if !everyone(dest) {
            to = ports.iter().filter(|p| p.knows(dest)).collect();
            if to.is_empty() {
                to = ports.iter().filter(|p| p.covers(dest)).collect();
            }
        }
        if to.is_empty() {
            to = ports.iter().collect();
        }
        for port in to {
            let _ = port.tx.try_send(packet.to_vec());
        }
    }
}

impl Port {
    pub fn address(&self) -> Ipv6Addr {
        self.tunnel.address
    }

    pub fn ifname(&self) -> Option<String> {
        self.tunnel.ifname()
    }

    pub fn owns(&self, address: Ipv6Addr) -> bool {
        self.tunnel.owns(address)
    }

    pub fn write(&self, packet: &[u8]) -> io::Result<()> {
        if let Some(source) = source(packet) {
            self.neighbours.lock().unwrap().insert(source);
        }
        match self.tunnel.fd() {
            Some(fd) => write_all(&fd, packet),
            None => Ok(()),
        }
    }

    pub fn recv(&self) -> Option<Vec<u8>> {
        self.rx.lock().unwrap().recv_timeout(POLL).ok()
    }

    pub fn set_v4(&self, v4: Option<(Ipv4Addr, u8)>) -> Result<bool, Error> {
        {
            let mut ours = self.v4.lock().unwrap();
            if *ours == v4 {
                return Ok(false);
            }
            *ours = v4;
        }
        let mut inner = self.tunnel.inner();
        self.tunnel.reopen(&mut inner).map(|()| true)
    }

    fn knows(&self, address: IpAddr) -> bool {
        self.neighbours.lock().unwrap().contains(&address)
    }

    fn covers(&self, address: IpAddr) -> bool {
        let (IpAddr::V4(address), Some((ours, prefix))) = (address, *self.v4.lock().unwrap())
        else {
            return false;
        };
        let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
        u32::from(address) & mask == u32::from(ours) & mask
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        let v4 = self.v4.lock().unwrap().is_some();
        self.tunnel.left(v4);
    }
}

fn read(tunnel: Weak<Tunnel>) {
    let mut buf = vec![0u8; FRAME_BUF];
    loop {
        let Some(tunnel) = tunnel.upgrade() else {
            return;
        };
        let Some(fd) = tunnel.fd() else {
            let mut inner = tunnel.inner();
            if inner.fd.is_none() {
                inner.reading = false;
                return;
            }
            continue;
        };
        match read_within(&fd, &mut buf) {
            Ok(Some(n)) => tunnel.dispatch(&buf[..n]),
            Ok(None) => {}
            Err(_) if tunnel.fd().is_some_and(|now| !Arc::ptr_eq(&now, &fd)) => {}
            Err(e) => {
                eprintln!("[tun] read ended: {e}");
                std::thread::sleep(POLL);
            }
        }
    }
}

fn destination(packet: &[u8]) -> Option<IpAddr> {
    match packet.first()? >> 4 {
        6 => <[u8; 16]>::try_from(packet.get(24..40)?)
            .ok()
            .map(|a| IpAddr::V6(a.into())),
        4 => <[u8; 4]>::try_from(packet.get(16..20)?)
            .ok()
            .map(|a| IpAddr::V4(a.into())),
        _ => None,
    }
}

fn source(packet: &[u8]) -> Option<IpAddr> {
    match packet.first()? >> 4 {
        6 => <[u8; 16]>::try_from(packet.get(8..24)?)
            .ok()
            .map(|a| IpAddr::V6(a.into())),
        4 => <[u8; 4]>::try_from(packet.get(12..16)?)
            .ok()
            .map(|a| IpAddr::V4(a.into())),
        _ => None,
    }
}

fn everyone(dest: IpAddr) -> bool {
    match dest {
        IpAddr::V6(a) => a.is_multicast(),
        IpAddr::V4(a) => a.is_multicast() || a.is_broadcast() || a.octets()[3] == 255,
    }
}

fn addresses_of(ifname: &str) -> Vec<Ipv6Addr> {
    super::android::ipv6_on(ifname)
}

fn made_up_mac() -> Mac {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x4c49_5649_4c49_4e4b);
    let b = seed.to_le_bytes();
    [0x02, b[0], b[1], b[2] ^ b[6], b[3] ^ b[5], b[4] ^ b[7]]
}
