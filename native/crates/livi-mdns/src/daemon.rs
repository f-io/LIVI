// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ffi::CStr;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::ExitCode;

use crate::wire::{self as mdns, Name};

const MAX_IF: usize = 8;
const POLL_MS: libc::c_int = 2000;

pub fn run(args: &[String]) -> ExitCode {
    let Some((host, ifnames)) = args.split_first() else {
        eprintln!("usage: mdnsd <name> <iface>...");
        return ExitCode::FAILURE;
    };
    if ifnames.is_empty() || ifnames.len() > MAX_IF {
        eprintln!("usage: mdnsd <name> <iface>...  (1..{MAX_IF} interfaces)");
        return ExitCode::FAILURE;
    }
    let name = Name::new(host);

    let fd = match bind_socket() {
        Ok(fd) => fd,
        Err(e) => {
            eprintln!("[mdnsd] {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut joined = vec![false; ifnames.len()];

    let mut packet = [0u8; 1500];
    loop {
        for (i, iface) in ifnames.iter().enumerate() {
            if joined[i] {
                continue;
            }
            let Some(addr) = iface_ipv4(iface) else {
                continue;
            };
            if !join_group(&fd, addr) {
                continue;
            }
            joined[i] = true;
            let announce = mdns::build_answer(&name, 0, addr, false, true, true);
            send_multicast(&fd, addr, &announce);
            println!("[mdnsd] {host}.local -> {addr} on {iface}");
        }

        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll is a valid pollfd and nfds is 1.
        if unsafe { libc::poll(&raw mut poll, 1, POLL_MS) } <= 0 {
            continue;
        }
        let Some((len, from, ifindex)) = receive(&fd, &mut packet) else {
            continue;
        };
        let Some(iface) = index_to_name(ifindex) else {
            continue;
        };
        if !ifnames.contains(&iface) {
            continue;
        }
        let Some(addr) = iface_ipv4(&iface) else {
            continue;
        };
        let Some(ask) = mdns::parse_query(&packet[..len], &name) else {
            continue;
        };

        let legacy = u16::from_be(from.sin_port) != mdns::PORT;
        let answer = mdns::build_answer(
            &name,
            if legacy { ask.id } else { 0 },
            addr,
            legacy,
            ask.want_a,
            ask.want_nsec,
        );
        if legacy || ask.unicast {
            send_to(&fd, &from, &answer);
        } else {
            send_multicast(&fd, addr, &answer);
        }
    }
}

fn bind_socket() -> Result<OwnedFd, String> {
    // SAFETY: socket has no memory preconditions.
    let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
    if raw < 0 {
        return Err(format!("socket: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: raw is a freshly created fd owned by nothing else.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let on: libc::c_int = 1;
    set_opt(&fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, &on);
    set_opt(&fd, libc::SOL_SOCKET, libc::SO_REUSEPORT, &on);
    set_opt(&fd, libc::IPPROTO_IP, libc::IP_PKTINFO, &on);
    let ttl: libc::c_uchar = 255;
    set_opt(&fd, libc::IPPROTO_IP, libc::IP_MULTICAST_TTL, &ttl);

    // SAFETY: an all-zero sockaddr_in is valid.
    let mut local: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    local.sin_family = libc::AF_INET as u16;
    local.sin_port = mdns::PORT.to_be();
    local.sin_addr.s_addr = libc::INADDR_ANY.to_be();
    // SAFETY: fd is open for the call and local is a valid sockaddr_in of the given size.
    let bound = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &raw const local as *const libc::sockaddr,
            size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    };
    if bound < 0 {
        return Err(format!(
            "bind :{}: {}",
            mdns::PORT,
            std::io::Error::last_os_error()
        ));
    }
    Ok(fd)
}

fn join_group(fd: &OwnedFd, via: Ipv4Addr) -> bool {
    let mreq = libc::ip_mreq {
        imr_multiaddr: libc::in_addr {
            s_addr: u32::from(mdns::GROUP).to_be(),
        },
        imr_interface: libc::in_addr {
            s_addr: u32::from(via).to_be(),
        },
    };
    // SAFETY: fd is open for the call and mreq is a valid ip_mreq of the given size.
    unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_ADD_MEMBERSHIP,
            &raw const mreq as *const libc::c_void,
            size_of::<libc::ip_mreq>() as libc::socklen_t,
        ) == 0
    }
}

fn receive(fd: &OwnedFd, buf: &mut [u8]) -> Option<(usize, libc::sockaddr_in, libc::c_uint)> {
    // SAFETY: an all-zero sockaddr_in is valid.
    let mut from: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    let mut control = [0u64; 32];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr() as *mut libc::c_void,
        iov_len: buf.len(),
    };
    // SAFETY: an all-zero msghdr is valid, all pointers in it are null.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_name = &raw mut from as *mut libc::c_void;
    msg.msg_namelen = size_of::<libc::sockaddr_in>() as libc::socklen_t;
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = size_of_val(&control) as _;

    // SAFETY: fd is open and msg points to from, iov over buf, and control, all live and
    // with matching lengths.
    let n = unsafe { libc::recvmsg(fd.as_raw_fd(), &raw mut msg, 0) };
    if n < 12 {
        return None;
    }
    let mut ifindex = 0;
    // SAFETY: msg is initialized by recvmsg and its control pointer and length describe control.
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !cmsg.is_null() {
        // SAFETY: cmsg is non-null and lies inside control, which is 8-aligned like cmsghdr.
        let header = unsafe { &*cmsg };
        if header.cmsg_level == libc::IPPROTO_IP && header.cmsg_type == libc::IP_PKTINFO {
            // SAFETY: cmsg is a non-null IP_PKTINFO header inside control, so its data holds an
            // in_pktinfo; read_unaligned covers the data offset's alignment.
            let info = unsafe {
                std::ptr::read_unaligned(libc::CMSG_DATA(cmsg).cast::<libc::in_pktinfo>())
            };
            ifindex = info.ipi_ifindex as libc::c_uint;
        }
        // SAFETY: cmsg is a valid header of msg, CMSG_NXTHDR stays within msg_controllen.
        cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
    }
    (ifindex != 0).then_some((n as usize, from, ifindex))
}

fn send_to(fd: &OwnedFd, to: &libc::sockaddr_in, data: &[u8]) {
    // SAFETY: fd is open, data is readable for data.len() bytes and to is a valid sockaddr_in.
    unsafe {
        libc::sendto(
            fd.as_raw_fd(),
            data.as_ptr() as *const libc::c_void,
            data.len(),
            0,
            to as *const libc::sockaddr_in as *const libc::sockaddr,
            size_of::<libc::sockaddr_in>() as libc::socklen_t,
        );
    }
}

fn send_multicast(fd: &OwnedFd, via: Ipv4Addr, data: &[u8]) {
    let iface = libc::in_addr {
        s_addr: u32::from(via).to_be(),
    };
    set_opt(fd, libc::IPPROTO_IP, libc::IP_MULTICAST_IF, &iface);
    // SAFETY: an all-zero sockaddr_in is valid.
    let mut group: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    group.sin_family = libc::AF_INET as u16;
    group.sin_port = mdns::PORT.to_be();
    group.sin_addr.s_addr = u32::from(mdns::GROUP).to_be();
    send_to(fd, &group, data);
}

fn set_opt<T>(fd: &OwnedFd, level: libc::c_int, option: libc::c_int, value: &T) {
    // SAFETY: fd is open for the call and value points to size_of::<T>() readable bytes.
    unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            level,
            option,
            value as *const T as *const libc::c_void,
            size_of::<T>() as libc::socklen_t,
        );
    }
}

fn iface_ipv4(name: &str) -> Option<Ipv4Addr> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: list is a valid out-pointer for getifaddrs.
    if unsafe { libc::getifaddrs(&raw mut list) } != 0 {
        return None;
    }
    let mut found = None;
    let mut entry = list;
    while !entry.is_null() {
        // SAFETY: entry is a non-null node of the getifaddrs list, which lives until freeifaddrs.
        let addrs = unsafe { &*entry };
        if !addrs.ifa_addr.is_null()
            // SAFETY: ifa_addr was checked non-null and points to a sockaddr of the live list.
            && unsafe { (*addrs.ifa_addr).sa_family } == libc::AF_INET as u16
            // SAFETY: ifa_name is a NUL-terminated string owned by the live list.
            && unsafe { CStr::from_ptr(addrs.ifa_name) }.to_str() == Ok(name)
        {
            // SAFETY: sa_family is AF_INET, so ifa_addr points to a sockaddr_in.
            let sin =
                unsafe { std::ptr::read_unaligned(addrs.ifa_addr.cast::<libc::sockaddr_in>()) };
            found = Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
            break;
        }
        entry = addrs.ifa_next;
    }
    // SAFETY: list came from a successful getifaddrs and is not used afterwards.
    unsafe { libc::freeifaddrs(list) };
    found
}

fn index_to_name(index: libc::c_uint) -> Option<String> {
    let mut buf: [libc::c_char; libc::IF_NAMESIZE] = [0; libc::IF_NAMESIZE];
    // SAFETY: buf holds IF_NAMESIZE bytes as if_indextoname requires.
    let name = unsafe { libc::if_indextoname(index, buf.as_mut_ptr()) };
    if name.is_null() {
        return None;
    }
    // SAFETY: name is non-null and points into buf, NUL-terminated by if_indextoname.
    unsafe { CStr::from_ptr(name) }
        .to_str()
        .ok()
        .map(str::to_string)
}
