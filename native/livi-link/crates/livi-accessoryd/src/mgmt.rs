//! The kernel does the pairing and keeps the link keys.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_HCI: libc::c_int = 1;
const HCI_CHANNEL_CONTROL: u16 = 3;
const HCI_DEV_NONE: u16 = 0xffff;

pub const INDEX: u16 = 0;
/// Largest management packet the kernel sends.
const PACKET_MAX: usize = 1024;
const HEADER: usize = 6;

pub const READ_VERSION: u16 = 0x0001;
pub const READ_COMMANDS: u16 = 0x0002;
pub const READ_INFO: u16 = 0x0004;
const READ_UNCONF_INDEX_LIST: u16 = 0x001d;
const READ_CONFIG_INFO: u16 = 0x0031;

pub const SET_POWERED: u16 = 0x0005;
pub const SET_DISCOVERABLE: u16 = 0x0006;
pub const SET_CONNECTABLE: u16 = 0x0007;
pub const SET_BONDABLE: u16 = 0x0009;
pub const SET_SSP: u16 = 0x000b;
pub const SET_CLASS: u16 = 0x000e;
pub const SET_NAME: u16 = 0x000f;
pub const ADD_UUID: u16 = 0x0010;
pub const REMOVE_UUID: u16 = 0x0011;
pub const LOAD_LINK_KEYS: u16 = 0x0012;
pub const DISCONNECT: u16 = 0x0014;
pub const PIN_CODE_NEG_REPLY: u16 = 0x0017;
pub const SET_IO_CAPABILITY: u16 = 0x0018;
pub const USER_CONFIRM_REPLY: u16 = 0x001c;

pub const NEEDED: [(u16, &str); 14] = [
    (SET_POWERED, "set-powered"),
    (SET_DISCOVERABLE, "set-discoverable"),
    (SET_CONNECTABLE, "set-connectable"),
    (SET_BONDABLE, "set-bondable"),
    (SET_SSP, "set-ssp"),
    (SET_CLASS, "set-class"),
    (SET_NAME, "set-name"),
    (ADD_UUID, "add-uuid"),
    (REMOVE_UUID, "remove-uuid"),
    (LOAD_LINK_KEYS, "load-link-keys"),
    (DISCONNECT, "disconnect"),
    (PIN_CODE_NEG_REPLY, "pin-code-neg-reply"),
    (SET_IO_CAPABILITY, "set-io-capability"),
    (USER_CONFIRM_REPLY, "user-confirm-reply"),
];

const CMD_COMPLETE: u16 = 0x0001;
const CMD_STATUS: u16 = 0x0002;

#[repr(C)]
struct SockaddrHci {
    family: libc::sa_family_t,
    dev: u16,
    channel: u16,
}

pub struct Mgmt {
    fd: OwnedFd,
}

impl Mgmt {
    pub fn open() -> Result<Self, String> {
        let raw =
            unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_RAW | libc::SOCK_CLOEXEC, BTPROTO_HCI) };
        if raw < 0 {
            return Err(format!("socket: {}", std::io::Error::last_os_error()));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let addr = SockaddrHci {
            family: AF_BLUETOOTH as libc::sa_family_t,
            dev: HCI_DEV_NONE,
            channel: HCI_CHANNEL_CONTROL,
        };
        let bound = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                &raw const addr as *const libc::sockaddr,
                size_of::<SockaddrHci>() as libc::socklen_t,
            )
        };
        if bound < 0 {
            return Err(format!("bind: {}", std::io::Error::last_os_error()));
        }
        Ok(Self { fd })
    }

    pub fn call(&self, opcode: u16, index: u16, params: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(HEADER + params.len());
        out.extend_from_slice(&opcode.to_le_bytes());
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(&(params.len() as u16).to_le_bytes());
        out.extend_from_slice(params);
        let mut sock = self.file();
        sock.write_all(&out).map_err(|e| format!("send {opcode:#06x}: {e}"))?;
        loop {
            let (event, _, body) = self.event()?;
            if body.len() < 3 {
                continue;
            }
            let answered = u16::from_le_bytes([body[0], body[1]]);
            if answered != opcode {
                continue;
            }
            let status = body[2];
            match event {
                CMD_COMPLETE if status == 0 => return Ok(body[3..].to_vec()),
                CMD_COMPLETE | CMD_STATUS => {
                    return Err(format!("{opcode:#06x} refused with status {status:#04x}"));
                }
                _ => continue,
            }
        }
    }

    pub fn event(&self) -> Result<(u16, u16, Vec<u8>), String> {
        let mut buf = [0u8; PACKET_MAX];
        let mut sock = self.file();
        let n = sock.read(&mut buf).map_err(|e| format!("receive: {e}"))?;
        if n < HEADER {
            return Err(format!("short packet of {n} bytes"));
        }
        let event = u16::from_le_bytes([buf[0], buf[1]]);
        let index = u16::from_le_bytes([buf[2], buf[3]]);
        let len = u16::from_le_bytes([buf[4], buf[5]]) as usize;
        if n < HEADER + len {
            return Err(format!("packet says {len} bytes, got {}", n - HEADER));
        }
        Ok((event, index, buf[HEADER..HEADER + len].to_vec()))
    }

    fn file(&self) -> std::mem::ManuallyDrop<std::fs::File> {
        std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(self.fd.as_raw_fd()) })
    }
}

pub struct Info {
    pub address: [u8; 6],
    pub supported: u32,
    pub current: u32,
    pub class: u32,
    pub name: String,
}

pub fn info(body: &[u8]) -> Result<Info, String> {
    if body.len() < 20 {
        return Err(format!("controller info is {} bytes", body.len()));
    }
    let mut address = [0u8; 6];
    address.copy_from_slice(&body[0..6]);
    let name = body
        .get(20..)
        .map(|n| String::from_utf8_lossy(n.split(|b| *b == 0).next().unwrap_or(&[])).into_owned())
        .unwrap_or_default();
    Ok(Info {
        address,
        supported: u32::from_le_bytes([body[9], body[10], body[11], body[12]]),
        current: u32::from_le_bytes([body[13], body[14], body[15], body[16]]),
        class: u32::from_le_bytes([body[17], body[18], body[19], 0]),
        name,
    })
}

pub fn commands(body: &[u8]) -> Vec<u16> {
    if body.len() < 4 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([body[0], body[1]]) as usize;
    body[4..].as_chunks::<2>().0.iter().take(count).map(|c| u16::from_le_bytes(*c)).collect()
}

/// The settings bits, in the order the kernel reports them.
pub const SETTINGS: [&str; 16] = [
    "powered",
    "connectable",
    "fast-connectable",
    "discoverable",
    "bondable",
    "link-security",
    "ssp",
    "br/edr",
    "hs",
    "le",
    "advertising",
    "secure-conn",
    "debug-keys",
    "privacy",
    "configuration",
    "static-address",
];

pub fn settings(bits: u32) -> String {
    SETTINGS
        .iter()
        .enumerate()
        .filter(|(i, _)| bits & (1 << i) != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn probe() -> std::process::ExitCode {
    let mgmt = match Mgmt::open() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[mgmt] {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match mgmt.call(READ_VERSION, HCI_DEV_NONE, &[]) {
        Ok(v) if v.len() >= 3 => {
            println!("[mgmt] version {}.{}", v[0], u16::from_le_bytes([v[1], v[2]]))
        }
        Ok(v) => println!("[mgmt] version reply of {} bytes", v.len()),
        Err(e) => {
            eprintln!("[mgmt] {e}");
            return std::process::ExitCode::FAILURE;
        }
    }
    match mgmt.call(READ_COMMANDS, HCI_DEV_NONE, &[]) {
        Ok(list) => {
            let known = commands(&list);
            let missing: Vec<&str> = NEEDED
                .iter()
                .filter(|(op, _)| !known.contains(op))
                .map(|(_, name)| *name)
                .collect();
            println!("[mgmt] {} commands known", known.len());
            if missing.is_empty() {
                println!("[mgmt] every command the accessory needs is there");
            } else {
                println!("[mgmt] missing: {}", missing.join(" "));
            }
        }
        Err(e) => eprintln!("[mgmt] {e}"),
    }
    match mgmt.call(READ_INFO, INDEX, &[]).and_then(|b| info(&b)) {
        Ok(i) => {
            let mac =
                i.address.iter().rev().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
            println!("[mgmt] controller {mac} class {:#08x} name {:?}", i.class, i.name);
            println!("[mgmt] supported: {}", settings(i.supported));
            println!("[mgmt] current:   {}", settings(i.current));
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[mgmt] {e}");
            unconfigured(&mgmt);
            std::process::ExitCode::FAILURE
        }
    }
}

/// A controller whose firmware carries no valid BD_ADDR stays unconfigured until a public address
/// is set.
fn unconfigured(mgmt: &Mgmt) {
    match mgmt.call(READ_UNCONF_INDEX_LIST, HCI_DEV_NONE, &[]) {
        Ok(list) if list.len() >= 2 => {
            let n = u16::from_le_bytes([list[0], list[1]]) as usize;
            let indexes: Vec<String> = list[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .take(n)
                .map(|c| u16::from_le_bytes(*c).to_string())
                .collect();
            println!(
                "[mgmt] unconfigured controllers: {}",
                if indexes.is_empty() { "none".to_string() } else { indexes.join(" ") }
            );
        }
        Ok(list) => println!("[mgmt] unconfigured list: {} byte reply", list.len()),
        Err(e) => eprintln!("[mgmt] unconfigured list: {e}"),
    }
    match mgmt.call(READ_CONFIG_INFO, INDEX, &[]) {
        Ok(b) if b.len() >= 10 => {
            let manufacturer = u16::from_le_bytes([b[0], b[1]]);
            let supported = u32::from_le_bytes([b[2], b[3], b[4], b[5]]);
            let missing = u32::from_le_bytes([b[6], b[7], b[8], b[9]]);
            println!(
                "[mgmt] config: manufacturer {manufacturer:#06x}, supported {}, missing {}",
                options(supported),
                options(missing)
            );
        }
        Ok(b) => println!("[mgmt] config info: {} byte reply", b.len()),
        Err(e) => eprintln!("[mgmt] config info: {e}"),
    }
}

fn options(bits: u32) -> String {
    let mut out = Vec::new();
    if (bits & 1) != 0 {
        out.push("external-config");
    }
    if (bits & 2) != 0 {
        out.push("public-address");
    }
    if out.is_empty() { "none".to_string() } else { out.join(" ") }
}
