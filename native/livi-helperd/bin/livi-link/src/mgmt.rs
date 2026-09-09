//! The controller's management socket. The kernel does the pairing and keeps the link keys, this
//! is where the accessory is configured and the kernel's questions are answered.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_HCI: libc::c_int = 1;
const HCI_CHANNEL_CONTROL: u16 = 3;
const HCI_DEV_NONE: u16 = 0xffff;

/// The controller every command in here is aimed at.
pub const INDEX: u16 = 0;
/// Largest management packet the kernel sends.
const PACKET_MAX: usize = 1024;
const HEADER: usize = 6;

pub const READ_VERSION: u16 = 0x0001;
pub const READ_COMMANDS: u16 = 0x0002;
pub const READ_INFO: u16 = 0x0004;

/// What the accessory needs from the kernel, by management opcode.
pub const NEEDED: [(u16, &str); 11] = [
    (0x0005, "set-powered"),
    (0x0006, "set-discoverable"),
    (0x0007, "set-connectable"),
    (0x0009, "set-bondable"),
    (0x000b, "set-ssp"),
    (0x000e, "set-class"),
    (0x000f, "set-name"),
    (0x0012, "load-link-keys"),
    (0x0018, "set-io-capability"),
    (0x0019, "pair-device"),
    (0x001c, "user-confirm-reply"),
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
    /// Opens the management socket, which is not tied to one controller.
    pub fn open() -> Result<Self, String> {
        let raw = unsafe {
            libc::socket(
                AF_BLUETOOTH,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                BTPROTO_HCI,
            )
        };
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

    /// Sends one command and returns what it answered, skipping unrelated events.
    pub fn call(&self, opcode: u16, index: u16, params: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(HEADER + params.len());
        out.extend_from_slice(&opcode.to_le_bytes());
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(&(params.len() as u16).to_le_bytes());
        out.extend_from_slice(params);
        let mut sock = self.file();
        sock.write_all(&out)
            .map_err(|e| format!("send {opcode:#06x}: {e}"))?;
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

    /// Reads one management packet: its event, the controller it is about, and its body.
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

/// What the controller says about itself.
pub struct Info {
    pub address: [u8; 6],
    pub supported: u32,
    pub current: u32,
    pub class: u32,
    pub name: String,
}

/// Reads the fixed head of a controller info reply.
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

/// The opcodes out of a command list reply.
pub fn commands(body: &[u8]) -> Vec<u16> {
    if body.len() < 4 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([body[0], body[1]]) as usize;
    body[4..]
        .as_chunks::<2>()
        .0
        .iter()
        .take(count)
        .map(|c| u16::from_le_bytes(*c))
        .collect()
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

/// Prints what the management socket says about the controller.
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
            println!(
                "[mgmt] version {}.{}",
                v[0],
                u16::from_le_bytes([v[1], v[2]])
            )
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
            let mac = i
                .address
                .iter()
                .rev()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(":");
            println!(
                "[mgmt] controller {mac} class {:#08x} name {:?}",
                i.class, i.name
            );
            println!("[mgmt] supported: {}", settings(i.supported));
            println!("[mgmt] current:   {}", settings(i.current));
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[mgmt] {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
