//! Userspace L2 bridge between two interfaces, for a kernel without CONFIG_BRIDGE. Both ends run
//! promiscuous and keep their own IP stack.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::ExitCode;

const QUEUE: usize = 1 << 20;
const FRAME_MAX: usize = 2048;

/// Shared receive ring.
const PACKET_VERSION: libc::c_int = 10;
const PACKET_RX_RING: libc::c_int = 5;
const TPACKET_V2: libc::c_int = 1;
const TP_STATUS_USER: u32 = 1;
const TPACKET2_HDRLEN: usize = 32;
const RING_FRAME: usize = FRAME_MAX;
const RING_BLOCK: usize = 8192;
const RING_BLOCKS: usize = 64;
const RING_FRAMES: usize = RING_BLOCK / RING_FRAME * RING_BLOCKS;

#[repr(C)]
struct TpacketReq {
    block_size: libc::c_uint,
    block_nr: libc::c_uint,
    frame_size: libc::c_uint,
    frame_nr: libc::c_uint,
}

#[repr(C)]
struct Tpacket2Hdr {
    status: u32,
    len: u32,
    snaplen: u32,
    mac: u16,
    net: u16,
    sec: u32,
    nsec: u32,
    vlan_tci: u16,
    vlan_tpid: u16,
    padding: [u8; 4],
}

/// Mapped ring and the next frame slot.
struct Ring {
    base: *mut u8,
    bytes: usize,
    next: usize,
}

impl Drop for Ring {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.base as *mut libc::c_void, self.bytes) };
    }
}

struct Iface {
    fd: OwnedFd,
    index: libc::c_int,
    name: String,
    mac: [u8; 6],
    ring: Option<Ring>,
}

/// Maps a receive ring on the socket. None when the kernel refuses it.
fn map_ring(fd: &OwnedFd) -> Option<Ring> {
    let version = TPACKET_V2;
    let set = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_PACKET,
            PACKET_VERSION,
            &raw const version as *const libc::c_void,
            size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if set < 0 {
        return None;
    }
    let req = TpacketReq {
        block_size: RING_BLOCK as libc::c_uint,
        block_nr: RING_BLOCKS as libc::c_uint,
        frame_size: RING_FRAME as libc::c_uint,
        frame_nr: RING_FRAMES as libc::c_uint,
    };
    let asked = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_PACKET,
            PACKET_RX_RING,
            &raw const req as *const libc::c_void,
            size_of::<TpacketReq>() as libc::socklen_t,
        )
    };
    if asked < 0 {
        return None;
    }
    let bytes = RING_BLOCK * RING_BLOCKS;
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            bytes,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )
    };
    if base == libc::MAP_FAILED {
        return None;
    }
    Some(Ring {
        base: base as *mut u8,
        bytes,
        next: 0,
    })
}

/// Forwards every frame the ring holds and releases each slot.
fn drain_ring(side: usize, ends: &mut [Iface; 2], _scratch: &mut [u8; FRAME_MAX]) {
    let out_index = ends[1 - side].index;
    let out_fd = ends[1 - side].fd.as_raw_fd();
    let own = ends[side].mac;
    let Some(ring) = ends[side].ring.as_mut() else {
        return;
    };
    loop {
        let slot = unsafe { ring.base.add(ring.next * RING_FRAME) };
        let header = slot as *mut Tpacket2Hdr;
        if unsafe { (*header).status } & TP_STATUS_USER == 0 {
            return;
        }
        let from = unsafe { slot.add(TPACKET2_HDRLEN) as *const libc::sockaddr_ll };
        let outgoing = unsafe { (*from).sll_pkttype } == libc::PACKET_OUTGOING;
        let at = unsafe { (*header).mac } as usize;
        let len = unsafe { (*header).snaplen } as usize;
        let bytes = unsafe { std::slice::from_raw_parts(slot.add(at), len) };
        // Skips our own transmissions and frames addressed to this interface.
        if !outgoing && len >= 6 && bytes[..6] != own {
            let mut to: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
            to.sll_family = libc::AF_PACKET as u16;
            to.sll_ifindex = out_index;
            to.sll_halen = 6;
            to.sll_addr[..6].copy_from_slice(&bytes[..6]);
            unsafe {
                libc::sendto(
                    out_fd,
                    bytes.as_ptr() as *const libc::c_void,
                    len,
                    0,
                    &raw const to as *const libc::sockaddr,
                    size_of::<libc::sockaddr_ll>() as libc::socklen_t,
                );
            }
        }
        unsafe { (*header).status = 0 };
        ring.next = (ring.next + 1) % RING_FRAMES;
    }
}

/// The interface's own address.
fn own_mac(name: &str) -> [u8; 6] {
    let mut mac = [0u8; 6];
    let Ok(text) = std::fs::read_to_string(format!("/sys/class/net/{name}/address")) else {
        return mac;
    };
    for (slot, byte) in mac.iter_mut().zip(text.trim().split(':')) {
        *slot = u8::from_str_radix(byte, 16).unwrap_or(0);
    }
    mac
}

fn open(name: &str) -> Result<Iface, String> {
    let cname = CString::new(name).map_err(|_| format!("bad interface name {name}"))?;
    let index = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    if index == 0 {
        return Err(format!("{name}: {}", std::io::Error::last_os_error()));
    }
    let index = index as libc::c_int;

    let raw = unsafe {
        libc::socket(
            libc::AF_PACKET,
            libc::SOCK_RAW,
            (libc::ETH_P_ALL as u16).to_be() as i32,
        )
    };
    if raw < 0 {
        return Err(format!("socket: {}", std::io::Error::last_os_error()));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };

    let mut mreq: libc::packet_mreq = unsafe { std::mem::zeroed() };
    mreq.mr_ifindex = index;
    mreq.mr_type = libc::PACKET_MR_PROMISC as u16;
    unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_PACKET,
            libc::PACKET_ADD_MEMBERSHIP,
            &raw const mreq as *const libc::c_void,
            size_of::<libc::packet_mreq>() as libc::socklen_t,
        );
    }

    for (name, size) in [(libc::SO_RCVBUF, QUEUE), (libc::SO_SNDBUF, QUEUE)] {
        let size = size as libc::c_int;
        unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                name,
                &raw const size as *const libc::c_void,
                size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
    }
    // Nonblocking, so each wake drains until empty.
    unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };

    let mut sll: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    sll.sll_family = libc::AF_PACKET as u16;
    sll.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
    sll.sll_ifindex = index;
    let bound = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &raw const sll as *const libc::sockaddr,
            size_of::<libc::sockaddr_ll>() as libc::socklen_t,
        )
    };
    if bound < 0 {
        return Err(format!("bind {name}: {}", std::io::Error::last_os_error()));
    }
    let ring = map_ring(&fd);
    Ok(Iface {
        fd,
        index,
        name: name.to_string(),
        mac: own_mac(name),
        ring,
    })
}

pub fn run(args: &[String]) -> ExitCode {
    let [a, b] = args else {
        eprintln!("usage: l2fwd <if1> <if2>");
        return ExitCode::FAILURE;
    };
    let mut ends = match (open(a), open(b)) {
        (Ok(a), Ok(b)) => [a, b],
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("[l2fwd] {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "[l2fwd] bridging {}({}) <-> {}({})",
        ends[0].name, ends[0].index, ends[1].name, ends[1].index
    );

    let mut fds = [
        libc::pollfd {
            fd: ends[0].fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: ends[1].fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let mut frame = [0u8; FRAME_MAX];
    loop {
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            continue;
        }
        for i in [0usize, 1] {
            if fds[i].revents & libc::POLLIN == 0 {
                continue;
            }
            let (input, output) = (&ends[i], &ends[1 - i]);
            if input.ring.is_some() {
                drain_ring(i, &mut ends, &mut frame);
                continue;
            }
            // Drain until empty before polling again.
            loop {
                let mut from: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
                let mut from_len = size_of::<libc::sockaddr_ll>() as libc::socklen_t;
                let n = unsafe {
                    libc::recvfrom(
                        input.fd.as_raw_fd(),
                        frame.as_mut_ptr() as *mut libc::c_void,
                        frame.len(),
                        0,
                        &raw mut from as *mut libc::sockaddr,
                        &raw mut from_len,
                    )
                };
                if n <= 0 {
                    break;
                }
                // Our own transmissions come back on the same socket.
                if from.sll_pkttype == libc::PACKET_OUTGOING {
                    continue;
                }
                // Addressed to this interface itself.
                if frame[..6] == input.mac {
                    continue;
                }
                let n = n as usize;
                let mut to: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
                to.sll_family = libc::AF_PACKET as u16;
                to.sll_ifindex = output.index;
                to.sll_halen = 6;
                to.sll_addr[..6].copy_from_slice(&frame[..6]);
                unsafe {
                    libc::sendto(
                        output.fd.as_raw_fd(),
                        frame.as_ptr() as *const libc::c_void,
                        n,
                        0,
                        &raw const to as *const libc::sockaddr,
                        size_of::<libc::sockaddr_ll>() as libc::socklen_t,
                    );
                }
            }
        }
    }
}
