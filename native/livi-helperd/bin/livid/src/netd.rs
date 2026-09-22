// Ported from bin/livi-netd/src/main.rs — see livid dispatcher in main.rs.
pub fn run(_args: Vec<String>) -> i32 {
    // ExitCode is opaque; the module either loops forever (returning implicitly)
    // or calls std::process::exit on setup failure. Treat any normal return as 0.
    let _ = livid_main();
    0
}

// livi-netd — DHCPv4 server for the LIVI-Link (V821B) dongle.
//
// - Serves DHCPv4 on the given interface: pool 10.10.10.100..149, gw/DNS = 10.10.10.1.
// - Delegates mDNS announce/response to the shared livi-mdns daemon (same code
//   powers CPC200's mdnsd, so we don't drift).
//
// Usage: livi-netd <iface> [server-ip] [pool-start] [pool-end] [hostname]

use std::env;
use std::io::{self, Result};
use std::net::Ipv4Addr;
use std::process::ExitCode;
use std::sync::Mutex;
use std::thread;

const DHCP_MAGIC: [u8; 4] = [99, 130, 83, 99];
const OPT_MSGTYPE: u8 = 53;
const OPT_SUBNET: u8 = 1;
const OPT_HOSTNAME: u8 = 12;
const OPT_LEASE: u8 = 51;
const OPT_SERVER: u8 = 54;
const OPT_END: u8 = 255;

const MSG_DISCOVER: u8 = 1;
const MSG_OFFER: u8 = 2;
const MSG_REQUEST: u8 = 3;
const MSG_ACK: u8 = 5;

fn livid_main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let iface = args.get(1).cloned().unwrap_or_else(|| "usb0".into());
    let server_ip: Ipv4Addr = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(Ipv4Addr::new(10, 10, 10, 1));
    let pool_start: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(100);
    let pool_end: u8 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(149);
    let hostname: String = args.get(5).cloned().unwrap_or_else(|| "livi-link".into());

    eprintln!(
        "[livi-netd] iface={iface} server={server_ip} pool=10.10.10.{pool_start}..{pool_end} host={hostname}.local"
    );

    // DHCP in a worker thread.
    let iface_c = iface.clone();
    thread::spawn(move || {
        if let Err(e) = dhcp_server(&iface_c, server_ip, pool_start, pool_end) {
            eprintln!("[livi-netd] dhcp: {e}");
        }
    });

    // Delegate mDNS to the shared crate. Its loop runs forever; if it ever returns,
    // the process exits with that code so init can respawn us.
    livi_mdns::daemon::run(&[hostname, iface])
}

// ============================================================================
// DHCPv4 server (unchanged from bespoke impl — busybox-udhcpd hits ENOPROTOOPT
// on this kernel/socket combo; UDP-only with SO_BINDTODEVICE + subnet-broadcast
// reply works for every common client (systemd-networkd, dhclient, udhcpc).
// ============================================================================

struct Leases {
    entries: Vec<([u8; 6], u8)>,
    next: u8,
    pool_start: u8,
    pool_end: u8,
}
impl Leases {
    fn new(pool_start: u8, pool_end: u8) -> Self {
        Self {
            entries: Vec::new(),
            next: pool_start,
            pool_start,
            pool_end,
        }
    }
    fn assign(&mut self, mac: [u8; 6]) -> u8 {
        if let Some(&(_, ip)) = self.entries.iter().find(|(m, _)| *m == mac) {
            return ip;
        }
        let ip = self.next;
        self.next = if self.next >= self.pool_end {
            self.pool_start
        } else {
            self.next + 1
        };
        self.entries.push((mac, ip));
        ip
    }
}

fn dhcp_server(iface: &str, server_ip: Ipv4Addr, pool_start: u8, pool_end: u8) -> Result<()> {
    unsafe {
        let sock = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if sock < 0 {
            return Err(io::Error::last_os_error());
        }
        let one: libc::c_int = 1;
        libc::setsockopt(
            sock,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &one as *const _ as *const _,
            size_of::<libc::c_int>() as u32,
        );
        libc::setsockopt(
            sock,
            libc::SOL_SOCKET,
            libc::SO_BROADCAST,
            &one as *const _ as *const _,
            size_of::<libc::c_int>() as u32,
        );
        let iface_bytes = iface.as_bytes();
        libc::setsockopt(
            sock,
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            iface_bytes.as_ptr() as *const _,
            iface_bytes.len() as u32,
        );

        let addr = libc::sockaddr_in {
            sin_family: libc::AF_INET as u16,
            sin_port: 67u16.to_be(),
            sin_addr: libc::in_addr { s_addr: 0 },
            sin_zero: [0; 8],
        };
        if libc::bind(sock, &addr as *const _ as *const _, size_of_val(&addr) as u32) < 0 {
            return Err(io::Error::last_os_error());
        }
        eprintln!("[livi-netd] dhcp bound to :67 on {iface}");

        let leases = Mutex::new(Leases::new(pool_start, pool_end));
        let mut buf = [0u8; 1500];
        loop {
            let mut from: libc::sockaddr_in = std::mem::zeroed();
            let mut fromlen = size_of::<libc::sockaddr_in>() as u32;
            let n = libc::recvfrom(
                sock,
                buf.as_mut_ptr() as *mut _,
                buf.len(),
                0,
                &mut from as *mut _ as *mut _,
                &mut fromlen,
            );
            if n < 240 {
                continue;
            }
            let n = n as usize;
            let pkt = &buf[..n];
            if pkt.len() < 240 || pkt[236..240] != DHCP_MAGIC {
                continue;
            }
            let op = pkt[0];
            if op != 1 {
                continue;
            }
            let xid = &pkt[4..8];
            let flags = &pkt[10..12];
            let chaddr: [u8; 6] = pkt[28..34].try_into().unwrap();

            let mut msgtype = 0u8;
            let mut i = 240;
            while i < pkt.len() {
                let code = pkt[i];
                if code == 0 {
                    i += 1;
                    continue;
                }
                if code == OPT_END {
                    break;
                }
                if i + 2 > pkt.len() {
                    break;
                }
                let len = pkt[i + 1] as usize;
                if i + 2 + len > pkt.len() {
                    break;
                }
                let val = &pkt[i + 2..i + 2 + len];
                if code == OPT_MSGTYPE && !val.is_empty() {
                    msgtype = val[0];
                }
                i += 2 + len;
            }
            if !(msgtype == MSG_DISCOVER || msgtype == MSG_REQUEST) {
                continue;
            }

            let last_octet = {
                let mut l = leases.lock().unwrap();
                l.assign(chaddr)
            };
            let so = server_ip.octets();
            let yiaddr = Ipv4Addr::new(so[0], so[1], so[2], last_octet);
            let reply_type = if msgtype == MSG_DISCOVER {
                MSG_OFFER
            } else {
                MSG_ACK
            };

            let out = build_reply(xid, flags, chaddr, yiaddr, server_ip, reply_type);
            let label = if reply_type == MSG_OFFER { "OFFER" } else { "ACK" };
            eprintln!("[livi-netd] dhcp {label} -> {yiaddr} mac={:02x?}", chaddr);

            let dst = libc::sockaddr_in {
                sin_family: libc::AF_INET as u16,
                sin_port: 68u16.to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_be_bytes([so[0], so[1], so[2], 255]).to_be(),
                },
                sin_zero: [0; 8],
            };
            libc::sendto(
                sock,
                out.as_ptr() as *const _,
                out.len(),
                0,
                &dst as *const _ as *const _,
                size_of_val(&dst) as u32,
            );
        }
    }
}

fn build_reply(
    xid: &[u8],
    flags: &[u8],
    chaddr: [u8; 6],
    yiaddr: Ipv4Addr,
    server_ip: Ipv4Addr,
    msgtype: u8,
) -> Vec<u8> {
    let mut b = vec![0u8; 240];
    b[0] = 2;
    b[1] = 1;
    b[2] = 6;
    b[3] = 0;
    b[4..8].copy_from_slice(xid);
    b[10..12].copy_from_slice(flags);
    b[16..20].copy_from_slice(&yiaddr.octets());
    b[20..24].copy_from_slice(&server_ip.octets());
    b[28..34].copy_from_slice(&chaddr);
    b[236..240].copy_from_slice(&DHCP_MAGIC);

    b.extend_from_slice(&[OPT_MSGTYPE, 1, msgtype]);
    b.extend_from_slice(&[OPT_SERVER, 4]);
    b.extend_from_slice(&server_ip.octets());
    b.extend_from_slice(&[OPT_LEASE, 4, 0, 0x01, 0x51, 0x80]);
    b.extend_from_slice(&[OPT_SUBNET, 4, 255, 255, 255, 0]);
    // No OPT_ROUTER / OPT_DNS: the dongle is neither a gateway nor a
    // resolver. The host must never route internet traffic through us.
    b.extend_from_slice(&[OPT_HOSTNAME, 9]);
    b.extend_from_slice(b"livi-link");
    b.push(OPT_END);
    while b.len() < 300 {
        b.push(0);
    }
    b
}
