#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(not(target_os = "macos"))]
pub fn wlan_mac(iface: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/address"))
        .ok()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
}

// macOS has no /sys; take the hardware address from getifaddrs' AF_LINK entry.
#[cfg(target_os = "macos")]
pub fn wlan_mac(iface: &str) -> Option<String> {
    let want = std::ffi::CString::new(iface).ok()?;
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
        return None;
    }
    let mut out = None;
    let mut cur = ifap;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
            continue;
        }
        if unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) } != want.as_c_str() {
            continue;
        }
        let sa = unsafe { &*ifa.ifa_addr };
        if i32::from(sa.sa_family) != libc::AF_LINK {
            continue;
        }
        let sdl = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_dl) };
        if sdl.sdl_alen != 6 {
            continue;
        }
        let base = sdl.sdl_nlen as usize;
        let mac: Vec<String> =
            (0..6).map(|i| format!("{:02X}", sdl.sdl_data[base + i] as u8)).collect();
        out = Some(mac.join(":"));
        break;
    }
    unsafe { libc::freeifaddrs(ifap) };
    out
}

#[cfg(target_os = "linux")]
pub fn wlan_link_local(iface: &str) -> Option<String> {
    let out = Command::new(crate::sys::tool("ip"))
        .args(["-6", "-o", "addr", "show", "dev", iface, "scope", "link"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .find(|t| t.starts_with("fe80:"))
        .map(|t| t.split('/').next().unwrap_or(t).to_string())
}

// Reads the interface's fe80 address from getifaddrs; the scope id BSD embeds in bytes 2-3
// of a link-local sockaddr (KAME) is zeroed.
#[cfg(target_os = "macos")]
pub fn wlan_link_local(iface: &str) -> Option<String> {
    let want = std::ffi::CString::new(iface).ok()?;
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
        return None;
    }
    let mut out = None;
    let mut cur = ifap;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
            continue;
        }
        if unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) } != want.as_c_str() {
            continue;
        }
        let sa = unsafe { &*ifa.ifa_addr };
        if i32::from(sa.sa_family) != libc::AF_INET6 {
            continue;
        }
        let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
        let mut a = sin6.sin6_addr.s6_addr;
        if a[0] == 0xfe && (a[1] & 0xc0) == 0x80 {
            a[2] = 0;
            a[3] = 0;
            out = Some(std::net::Ipv6Addr::from(a).to_string());
            break;
        }
    }
    unsafe { libc::freeifaddrs(ifap) };
    out
}

/// Local interface sharing a /24 with `peer` ("host" or "host:port").
pub fn iface_facing(peer: &str) -> Option<String> {
    facing(peer).map(|(name, _)| name)
}

/// The IPv4 address of `iface`.
pub fn ipv4_of(iface: &str) -> Option<std::net::Ipv4Addr> {
    first_ipv4(|name, _| name == iface).map(|(_, addr)| addr)
}

fn facing(peer: &str) -> Option<(String, std::net::Ipv4Addr)> {
    use std::net::ToSocketAddrs;
    let host = peer.rsplit_once(':').map(|(h, _)| h).unwrap_or(peer);
    let ip: std::net::Ipv4Addr = match host.parse() {
        Ok(ip) => ip,
        Err(_) => (host, 0u16).to_socket_addrs().ok()?.find_map(|a| match a {
            std::net::SocketAddr::V4(v4) => Some(*v4.ip()),
            std::net::SocketAddr::V6(_) => None,
        })?,
    };
    let subnet = u32::from(ip) & 0xffff_ff00;
    first_ipv4(|_, own| u32::from(own) & 0xffff_ff00 == subnet)
}

/// The first interface IPv4 address `wanted` accepts.
fn first_ipv4(
    wanted: impl Fn(&str, std::net::Ipv4Addr) -> bool,
) -> Option<(String, std::net::Ipv4Addr)> {
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
        return None;
    }
    let mut out = None;
    let mut cur = ifap;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
            continue;
        }
        let sa = unsafe { &*ifa.ifa_addr };
        if i32::from(sa.sa_family) != libc::AF_INET {
            continue;
        }
        let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
        let own = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
        if let Ok(name) = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }.to_str()
            && wanted(name, own)
        {
            out = Some((name.to_string(), own));
            break;
        }
    }
    unsafe { libc::freeifaddrs(ifap) };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loopback_interface_has_its_address() {
        let lo = if cfg!(target_os = "macos") { "lo0" } else { "lo" };
        assert_eq!(ipv4_of(lo), Some(std::net::Ipv4Addr::LOCALHOST));
        assert_eq!(ipv4_of("no-such-iface"), None);
    }
}
