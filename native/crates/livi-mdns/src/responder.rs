// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ffi::CString;
use std::io;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};

use crate::service::Service;
use crate::wire::PORT;

const GROUP: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xfb);
const HOPS: u32 = 255;
const ANNOUNCEMENTS: u32 = 3;
const WAKE: Duration = Duration::from_secs(1);

pub struct Responder {
    run: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Responder {
    pub fn start(ifname: &str, service: Service) -> io::Result<Self> {
        let name = CString::new(ifname).map_err(io::Error::other)?;
        let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
        if index == 0 {
            return Err(io::Error::last_os_error());
        }
        let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        socket.set_only_v6(true)?;
        socket.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, PORT)).into())?;
        socket.join_multicast_v6(&GROUP, index)?;
        socket.set_multicast_if_v6(index)?;
        socket.set_multicast_hops_v6(HOPS)?;
        socket.set_read_timeout(Some(WAKE))?;
        let socket: UdpSocket = socket.into();
        let group = SocketAddrV6::new(GROUP, PORT, 0, index);
        let run = Arc::new(AtomicBool::new(true));
        let thread = std::thread::Builder::new().name("mdns".into()).spawn({
            let run = run.clone();
            move || serve(&socket, group, &service, &run)
        })?;
        Ok(Self {
            run,
            thread: Some(thread),
        })
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.run.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(socket: &UdpSocket, group: SocketAddrV6, service: &Service, run: &AtomicBool) {
    let records = service.records();
    let mut announced = 0;
    let mut packet = [0u8; 1500];
    while run.load(Ordering::SeqCst) {
        if announced < ANNOUNCEMENTS {
            let _ = socket.send_to(&records, group);
            announced += 1;
        }
        let Ok((len, from)) = socket.recv_from(&mut packet) else {
            continue;
        };
        match service.asked(&packet[..len]) {
            Some(true) => {
                let _ = socket.send_to(&records, from);
            }
            Some(false) => {
                let _ = socket.send_to(&records, group);
            }
            None => {}
        }
    }
}
