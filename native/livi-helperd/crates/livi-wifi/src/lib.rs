#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

//! What the radio may transmit on, asked over nl80211 the way `iw list` does. The answer already
//! reflects the country hostapd asked for.

/// The stub for a host without nl80211.
#[cfg(not(target_os = "linux"))]
pub fn listing() -> Result<String, String> {
    Err("the channel list needs linux".into())
}

/// What an interface beacons: its network, channel and width in MHz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApState {
    pub ssid: String,
    pub channel: u32,
    pub width: u32,
}

/// The stub for a host without nl80211.
#[cfg(not(target_os = "linux"))]
pub fn ap_state(_iface: &str) -> Option<ApState> {
    None
}

#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(target_os = "linux")]
use std::process::ExitCode;

#[cfg(target_os = "linux")]
const NETLINK_GENERIC: libc::c_int = 16;
const GENL_ID_CTRL: u16 = 0x10;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;

const NL80211_CMD_GET_WIPHY: u8 = 1;
const NL80211_CMD_GET_REG: u8 = 31;
const NL80211_CMD_GET_INTERFACE: u8 = 5;
const ATTR_IFNAME: u16 = 4;
const ATTR_IFTYPE: u16 = 5;
const ATTR_WIPHY_FREQ: u16 = 38;
const ATTR_SSID: u16 = 52;
const ATTR_CHANNEL_WIDTH: u16 = 159;
const IFTYPE_AP: u32 = 3;
const ATTR_WIPHY: u16 = 1;
const ATTR_WIPHY_NAME: u16 = 2;
const ATTR_WIPHY_BANDS: u16 = 22;
const ATTR_REG_ALPHA2: u16 = 33;
const ATTR_SPLIT_WIPHY_DUMP: u16 = 174;
const BAND_ATTR_FREQS: u16 = 1;
const FREQ_ATTR_FREQ: u16 = 1;
const FREQ_ATTR_DISABLED: u16 = 2;
// Called PASSIVE_SCAN on this kernel, NO_IR since 3.15.
const FREQ_ATTR_NO_IR: u16 = 3;
const FREQ_ATTR_RADAR: u16 = 5;
const FREQ_ATTR_MAX_TX_POWER: u16 = 6;

const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 0x001;
const NLM_F_ACK: u16 = 0x004;
const NLM_F_DUMP: u16 = 0x300;

/// The netlink header plus the generic netlink header in front of every payload.
const HDR: usize = 20;

#[cfg(target_os = "linux")]
pub fn run() -> ExitCode {
    match listing() {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[wifi] {e}");
            ExitCode::FAILURE
        }
    }
}

/// The regulatory country and every channel the radio knows, one per line, with the flags the
/// kernel put on it.
#[cfg(target_os = "linux")]
pub fn listing() -> Result<String, String> {
    let fd = open()?;
    let family = family_id(&fd)?;
    let mut out = String::new();
    if let Ok(country) = country(&fd, family) {
        out.push_str(&format!("country {country}\n"));
    }
    for radio in radios(&fd, family)? {
        out.push_str(&format!("phy {}\n", radio.name));
        for c in radio.channels {
            if let Some(ch) = channel_of(c.freq) {
                out.push_str(&format!("chan {ch} {} {} {}\n", c.freq, c.flags, c.dbm));
            }
        }
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
fn open() -> Result<OwnedFd, String> {
    let raw = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            NETLINK_GENERIC,
        )
    };
    if raw < 0 {
        return Err(format!(
            "netlink socket: {}",
            std::io::Error::last_os_error()
        ));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut local: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    local.nl_family = libc::AF_NETLINK as u16;
    let bound = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &raw const local as *const libc::sockaddr,
            size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if bound < 0 {
        return Err(format!("netlink bind: {}", std::io::Error::last_os_error()));
    }
    // Receive timeout.
    let timeout = libc::timeval {
        tv_sec: 3,
        tv_usec: 0,
    };
    unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &raw const timeout as *const libc::c_void,
            size_of::<libc::timeval>() as libc::socklen_t,
        );
    }
    Ok(fd)
}

#[cfg(target_os = "linux")]
fn family_id(fd: &OwnedFd) -> Result<u16, String> {
    let name = attr(CTRL_ATTR_FAMILY_NAME, b"nl80211\0");
    let request = message(GENL_ID_CTRL, CTRL_CMD_GETFAMILY, NLM_F_ACK, &name);
    for payload in call(fd, &request)? {
        for (kind, value) in Attrs(&payload[..]) {
            if kind == CTRL_ATTR_FAMILY_ID && value.len() >= 2 {
                return Ok(u16::from_ne_bytes([value[0], value[1]]));
            }
        }
    }
    Err("nl80211 is not registered with generic netlink".into())
}

/// What an interface beacons right now, None until it is an access point with a network
/// up. Read from the kernel, so it holds what is on air rather than what a config asked for.
#[cfg(target_os = "linux")]
pub fn ap_state(iface: &str) -> Option<ApState> {
    let fd = open().ok()?;
    let family = family_id(&fd).ok()?;
    let request = message(family, NL80211_CMD_GET_INTERFACE, NLM_F_DUMP, &[]);
    for payload in call(&fd, &request).ok()? {
        let mut name = None;
        let mut kind_of = None;
        let mut freq = None;
        let mut ssid = None;
        let mut width = 0;
        for (kind, value) in Attrs(&payload[..]) {
            let u32_of = |v: &[u8]| u32::from_ne_bytes([v[0], v[1], v[2], v[3]]);
            match kind {
                ATTR_IFNAME => name = Some(text(value)),
                ATTR_SSID => ssid = Some(text(value)),
                ATTR_IFTYPE if value.len() >= 4 => kind_of = Some(u32_of(value)),
                ATTR_WIPHY_FREQ if value.len() >= 4 => freq = Some(u32_of(value)),
                ATTR_CHANNEL_WIDTH if value.len() >= 4 => width = width_mhz(u32_of(value)),
                _ => {}
            }
        }
        if name.as_deref() == Some(iface) && kind_of == Some(IFTYPE_AP) {
            let ssid = ssid.filter(|s| !s.is_empty())?;
            return Some(ApState { ssid, channel: channel_of(freq?)?, width });
        }
    }
    None
}

/// `enum nl80211_chan_width` as MHz. 80+80 reports its primary segment.
fn width_mhz(raw: u32) -> u32 {
    match raw {
        0 | 1 => 20,
        2 => 40,
        3 | 4 => 80,
        5 => 160,
        13 => 320,
        _ => 0,
    }
}

#[cfg(target_os = "linux")]
fn country(fd: &OwnedFd, family: u16) -> Result<String, String> {
    let request = message(family, NL80211_CMD_GET_REG, NLM_F_ACK, &[]);
    for payload in call(fd, &request)? {
        for (kind, value) in Attrs(&payload[..]) {
            if kind == ATTR_REG_ALPHA2 && value.len() >= 2 {
                return Ok(text(&value[..2]));
            }
        }
    }
    Err("no regulatory domain".into())
}

/// One radio and the channels the kernel reports for it.
#[cfg(target_os = "linux")]
struct Radio {
    id: u32,
    name: String,
    channels: Vec<Channel>,
}

/// One frequency as the kernel describes it. `dbm` is the permitted EIRP, 0 when it says
/// nothing.
struct Channel {
    freq: u32,
    flags: String,
    dbm: u32,
}

/// Every radio the kernel knows, kept apart. The dump is asked for split.
#[cfg(target_os = "linux")]
fn radios(fd: &OwnedFd, family: u16) -> Result<Vec<Radio>, String> {
    let split = attr(ATTR_SPLIT_WIPHY_DUMP, &[]);
    let request = message(family, NL80211_CMD_GET_WIPHY, NLM_F_DUMP, &split);
    let mut radios: Vec<Radio> = Vec::new();
    for payload in call(fd, &request)? {
        let mut id = None;
        let mut name = None;
        let mut found = Vec::new();
        for (kind, value) in Attrs(&payload[..]) {
            match kind {
                ATTR_WIPHY if value.len() >= 4 => {
                    id = Some(u32::from_ne_bytes([value[0], value[1], value[2], value[3]]));
                }
                ATTR_WIPHY_NAME => name = Some(text(value)),
                ATTR_WIPHY_BANDS => {
                    for (_, band) in Attrs(value) {
                        for (kind, freqs) in Attrs(band) {
                            if kind == BAND_ATTR_FREQS {
                                found.extend(Attrs(freqs).filter_map(|(_, f)| frequency(f)));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let Some(id) = id else { continue };
        let at = match radios.iter().position(|r| r.id == id) {
            Some(at) => at,
            None => {
                radios.push(Radio {
                    id,
                    name: String::new(),
                    channels: Vec::new(),
                });
                radios.len() - 1
            }
        };
        if let Some(name) = name {
            radios[at].name = name;
        }
        radios[at].channels.extend(found);
    }
    for radio in &mut radios {
        radio.channels.sort_by_key(|c| c.freq);
        radio.channels.dedup_by_key(|c| c.freq);
    }
    radios.retain(|radio| !radio.channels.is_empty());
    Ok(radios)
}

/// A netlink string, which carries its terminator.
fn text(value: &[u8]) -> String {
    let end = value.iter().position(|b| *b == 0).unwrap_or(value.len());
    String::from_utf8_lossy(&value[..end]).into_owned()
}

/// One frequency with the flags the kernel put on it. Power comes in mBm.
fn frequency(attrs: &[u8]) -> Option<Channel> {
    let mut freq = None;
    let mut dbm = 0;
    let mut flags = Vec::new();
    for (kind, value) in Attrs(attrs) {
        match kind {
            FREQ_ATTR_FREQ if value.len() >= 4 => {
                freq = Some(u32::from_ne_bytes([value[0], value[1], value[2], value[3]]));
            }
            FREQ_ATTR_MAX_TX_POWER if value.len() >= 4 => {
                dbm = u32::from_ne_bytes([value[0], value[1], value[2], value[3]]) / 100;
            }
            FREQ_ATTR_DISABLED => flags.push("disabled"),
            FREQ_ATTR_NO_IR => flags.push("no-ir"),
            FREQ_ATTR_RADAR => flags.push("radar"),
            _ => {}
        }
    }
    if flags.is_empty() {
        flags.push("ok");
    }
    Some(Channel { freq: freq?, flags: flags.join(","), dbm })
}

/// The channel number for a frequency, the way hostapd wants it written.
fn channel_of(freq: u32) -> Option<u32> {
    match freq {
        2484 => Some(14),
        2412..=2472 => Some((freq - 2407) / 5),
        5000..=5895 => Some((freq - 5000) / 5),
        _ => None,
    }
}

/// One request with its generic netlink header, ready to send.
fn message(family: u16, cmd: u8, flags: u16, attrs: &[u8]) -> Vec<u8> {
    let len = HDR + attrs.len();
    let mut m = Vec::with_capacity(len);
    m.extend_from_slice(&(len as u32).to_ne_bytes());
    m.extend_from_slice(&family.to_ne_bytes());
    m.extend_from_slice(&(flags | NLM_F_REQUEST).to_ne_bytes());
    m.extend_from_slice(&1u32.to_ne_bytes());
    // The kernel fills in the port id.
    m.extend_from_slice(&0u32.to_ne_bytes());
    m.push(cmd);
    m.push(1);
    m.extend_from_slice(&0u16.to_ne_bytes());
    m.extend_from_slice(attrs);
    m
}

fn attr(kind: u16, payload: &[u8]) -> Vec<u8> {
    let len = 4 + payload.len();
    let mut a = Vec::with_capacity(align(len));
    a.extend_from_slice(&(len as u16).to_ne_bytes());
    a.extend_from_slice(&kind.to_ne_bytes());
    a.extend_from_slice(payload);
    a.resize(align(len), 0);
    a
}

const fn align(n: usize) -> usize {
    (n + 3) & !3
}

/// Sends one request and hands back the payload of every answer, up to the end of the dump.
#[cfg(target_os = "linux")]
fn call(fd: &OwnedFd, request: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let sent = unsafe {
        libc::send(
            fd.as_raw_fd(),
            request.as_ptr() as *const libc::c_void,
            request.len(),
            0,
        )
    };
    if sent < 0 {
        return Err(format!("netlink send: {}", std::io::Error::last_os_error()));
    }

    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let got = unsafe {
            libc::recv(
                fd.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
            )
        };
        if got <= 0 {
            return Err(format!("netlink recv: {}", std::io::Error::last_os_error()));
        }
        let mut rest = &buf[..got as usize];
        while rest.len() >= 16 {
            let len = u32::from_ne_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            let kind = u16::from_ne_bytes([rest[4], rest[5]]);
            if len < 16 || len > rest.len() {
                return Err("truncated netlink message".into());
            }
            match kind {
                NLMSG_DONE => return Ok(out),
                NLMSG_ERROR => {
                    let code = i32::from_ne_bytes([rest[16], rest[17], rest[18], rest[19]]);
                    if code != 0 {
                        return Err(format!("netlink error {code}"));
                    }
                    // A zero is the acknowledgement, which ends a plain request.
                    return Ok(out);
                }
                _ if len > HDR => out.push(rest[HDR..len].to_vec()),
                _ => {}
            }
            rest = &rest[align(len).min(rest.len())..];
        }
    }
}

/// Walks a netlink attribute list.
struct Attrs<'a>(&'a [u8]);

impl<'a> Iterator for Attrs<'a> {
    type Item = (u16, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.len() < 4 {
            return None;
        }
        let len = u16::from_ne_bytes([self.0[0], self.0[1]]) as usize;
        let kind = u16::from_ne_bytes([self.0[2], self.0[3]]);
        if len < 4 || len > self.0.len() {
            return None;
        }
        let value = &self.0[4..len];
        self.0 = &self.0[align(len).min(self.0.len())..];
        // The nested and byte order bits are not part of the type.
        Some((kind & 0x3fff, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frequency_maps_to_the_channel_hostapd_wants() {
        assert_eq!(channel_of(2412), Some(1));
        assert_eq!(channel_of(2472), Some(13));
        assert_eq!(channel_of(2484), Some(14));
        assert_eq!(channel_of(5180), Some(36));
        assert_eq!(channel_of(5825), Some(165));
        assert_eq!(channel_of(1000), None);
    }

    #[test]
    fn attributes_are_walked_with_their_padding() {
        let mut list = attr(1, &[0xaa]);
        list.extend(attr(2, &[1, 2, 3, 4]));
        let seen: Vec<_> = Attrs(&list[..])
            .map(|(kind, value)| (kind, value.to_vec()))
            .collect();
        assert_eq!(seen, vec![(1, vec![0xaa]), (2, vec![1, 2, 3, 4])]);
    }

    #[test]
    fn a_string_stops_at_its_terminator() {
        assert_eq!(text(b"phy0\0"), "phy0");
        assert_eq!(text(b"phy0"), "phy0");
    }

    #[test]
    fn a_nested_type_keeps_only_its_number() {
        let list = attr(0x8000 | 22, &[]);
        let (kind, value) = Attrs(&list[..]).next().unwrap();
        assert_eq!(kind, 22);
        assert!(value.is_empty());
    }

    #[test]
    fn a_channel_carries_the_flags_and_the_power_the_kernel_set() {
        let mut usable = attr(FREQ_ATTR_FREQ, &5180u32.to_ne_bytes());
        usable.extend(attr(FREQ_ATTR_MAX_TX_POWER, &2000u32.to_ne_bytes()));
        let usable = frequency(&usable).unwrap();
        assert_eq!((usable.freq, usable.flags.as_str(), usable.dbm), (5180, "ok", 20));

        let mut dfs = attr(FREQ_ATTR_FREQ, &5260u32.to_ne_bytes());
        dfs.extend(attr(FREQ_ATTR_NO_IR, &[]));
        dfs.extend(attr(FREQ_ATTR_RADAR, &[]));
        let dfs = frequency(&dfs).unwrap();
        assert_eq!((dfs.freq, dfs.flags.as_str(), dfs.dbm), (5260, "no-ir,radar", 0));

        assert!(frequency(&attr(FREQ_ATTR_DISABLED, &[])).is_none());
    }

    #[test]
    fn a_request_carries_its_length_and_command() {
        let m = message(
            0x10,
            CTRL_CMD_GETFAMILY,
            NLM_F_ACK,
            &attr(CTRL_ATTR_FAMILY_NAME, b"nl80211\0"),
        );
        assert_eq!(
            u32::from_ne_bytes([m[0], m[1], m[2], m[3]]) as usize,
            m.len()
        );
        assert_eq!(u16::from_ne_bytes([m[4], m[5]]), 0x10);
        assert_eq!(u16::from_ne_bytes([m[6], m[7]]), NLM_F_ACK | NLM_F_REQUEST);
        assert_eq!(m[16], CTRL_CMD_GETFAMILY);
        assert_eq!(&m[HDR + 4..], b"nl80211\0");
    }

    #[test]
    fn a_channel_width_comes_back_in_megahertz() {
        assert_eq!(super::width_mhz(1), 20);
        assert_eq!(super::width_mhz(2), 40);
        assert_eq!(super::width_mhz(3), 80);
        assert_eq!(super::width_mhz(5), 160);
        assert_eq!(super::width_mhz(99), 0);
    }
}
