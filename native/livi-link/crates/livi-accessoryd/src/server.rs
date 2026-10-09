use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, OnceLock};

use livi_net::port::{ACCESSORY, IAP};

use crate::mgmt::{
    self, ADD_UUID, DISCONNECT, LOAD_LINK_KEYS, Mgmt, PIN_CODE_NEG_REPLY, REMOVE_UUID,
    SET_BONDABLE, SET_CLASS, SET_CONNECTABLE, SET_DISCOVERABLE, SET_IO_CAPABILITY, SET_NAME,
    SET_POWERED, SET_SSP, USER_CONFIRM_REPLY,
};
use crate::sdp;

pub type NameSource = Arc<dyn Fn() -> Option<String> + Send + Sync>;

static KEYS_PATH: OnceLock<String> = OnceLock::new();
static NAME_SOURCE: OnceLock<NameSource> = OnceLock::new();

fn keys_path() -> String {
    KEYS_PATH.get().cloned().unwrap_or_default()
}

fn wifid_ap_name() -> Option<String> {
    NAME_SOURCE.get().and_then(|f| f())
}

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_RFCOMM: libc::c_int = 3;
/// One stored bond: the address, its kind, the key and its length.
const KEY_LEN: usize = 25;

#[repr(C)]
struct SockaddrRc {
    family: libc::sa_family_t,
    bdaddr: [u8; 6],
    channel: u8,
}

const NAME: &str = "LIVI Link";
const NAME_POLL: std::time::Duration = std::time::Duration::from_secs(5);
const CONTROLLER_TRIES: u32 = 120;
const CONTROLLER_POLL: std::time::Duration = std::time::Duration::from_millis(500);
/// Audio/Video, car audio.
const CLASS_MAJOR: u8 = 0x04;
const CLASS_MINOR: u8 = 0x20;
const SERVICE_AUDIO: u8 = 0x20;
const IO_NO_INPUT_NO_OUTPUT: u8 = 0x03;

const EV_NEW_SETTINGS: u16 = 0x0006;
const EV_NEW_LINK_KEY: u16 = 0x0009;
const EV_DEVICE_CONNECTED: u16 = 0x000b;
const EV_DEVICE_DISCONNECTED: u16 = 0x000c;
const EV_CONNECT_FAILED: u16 = 0x000d;
const EV_PIN_CODE_REQUEST: u16 = 0x000e;
const EV_USER_CONFIRM_REQUEST: u16 = 0x000f;
const EV_AUTH_FAILED: u16 = 0x0011;

pub struct Config {
    pub keys_path: String,
    pub ap_name: NameSource,
}

pub fn run(config: Config) -> ExitCode {
    let _ = KEYS_PATH.set(config.keys_path.clone());
    let _ = NAME_SOURCE.set(config.ap_name.clone());
    let name = wifid_ap_name().unwrap_or_else(|| NAME.into());
    let name = name.as_str();
    let Some((mgmt, local)) = ready() else {
        eprintln!("[accessory] the controller never answered");
        return ExitCode::FAILURE;
    };
    println!("[accessory] {name} stays off the air until the host asks for it");
    std::thread::spawn(control);
    let mut shown = name.to_string();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(NAME_POLL);
            let Some(current) = wifid_ap_name() else {
                continue;
            };
            if current == shown {
                continue;
            }
            match Mgmt::open()
                .and_then(|m| m.call(SET_NAME, mgmt::INDEX, &local_name(&current)).map(|_| ()))
            {
                Ok(()) => {
                    println!("[accessory] the car is now called {current}");
                    shown = current;
                }
                Err(e) => eprintln!("[accessory] renaming to {current}: {e}"),
            }
        }
    });
    std::thread::spawn(|| {
        if let Err(e) = sdp::serve() {
            eprintln!("[sdp] {e}");
        }
    });
    std::thread::spawn(move || {
        if let Err(e) = channel(&local) {
            eprintln!("[accessory] {e}");
        }
    });
    listen(&mgmt);
    ExitCode::SUCCESS
}

fn ready() -> Option<(Mgmt, String)> {
    for _ in 0..CONTROLLER_TRIES {
        if let Ok(mgmt) = Mgmt::open()
            && let Ok(info) =
                mgmt.call(mgmt::READ_INFO, mgmt::INDEX, &[]).and_then(|b| mgmt::info(&b))
        {
            let local = mac(&info.address);
            return Some((mgmt, local));
        }
        std::thread::sleep(CONTROLLER_POLL);
    }
    None
}

fn present(mgmt: &Mgmt, name: &str) -> Result<(), String> {
    mgmt.call(SET_POWERED, mgmt::INDEX, &[1])?;
    mgmt.call(SET_SSP, mgmt::INDEX, &[1])?;
    mgmt.call(SET_IO_CAPABILITY, mgmt::INDEX, &[IO_NO_INPUT_NO_OUTPUT])?;
    mgmt.call(SET_BONDABLE, mgmt::INDEX, &[1])?;
    mgmt.call(SET_CLASS, mgmt::INDEX, &[CLASS_MAJOR, CLASS_MINOR])?;
    mgmt.call(SET_NAME, mgmt::INDEX, &local_name(name))?;
    mgmt.call(SET_CONNECTABLE, mgmt::INDEX, &[1])?;
    // A timeout of zero stays visible.
    let mut visible = vec![1u8];
    visible.extend_from_slice(&0u16.to_le_bytes());
    mgmt.call(SET_DISCOVERABLE, mgmt::INDEX, &visible)?;
    Ok(())
}

fn channel(local: &str) -> Result<(), String> {
    let host: Arc<Mutex<Option<TcpStream>>> = Arc::default();
    let attending = host.clone();
    std::thread::spawn(move || attend(&attending));
    let mut open = Vec::new();
    for record in &sdp::RECORDS {
        let listener = rfcomm(record.channel)?;
        let (local, host) = (local.to_string(), host.clone());
        open.push(std::thread::spawn(move || take(&listener, record, &local, &host)));
    }
    println!("[accessory] waiting for a phone, host on :{IAP}");
    for thread in open {
        let _ = thread.join();
    }
    Ok(())
}

fn accept(listener: &OwnedFd) -> std::io::Result<(std::fs::File, [u8; 6])> {
    let mut peer = SockaddrRc { family: 0, bdaddr: [0; 6], channel: 0 };
    let mut size = size_of::<SockaddrRc>() as libc::socklen_t;
    let raw = unsafe {
        libc::accept(listener.as_raw_fd(), &raw mut peer as *mut libc::sockaddr, &raw mut size)
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((std::fs::File::from(unsafe { OwnedFd::from_raw_fd(raw) }), peer.bdaddr))
}

fn take(listener: &OwnedFd, record: &sdp::Record, local: &str, host: &Mutex<Option<TcpStream>>) {
    let (channel, name) = (record.channel, record.name);
    loop {
        let (link, phone) = match accept(listener) {
            Ok(accepted) => accepted,
            Err(e) => {
                eprintln!("[accessory] accept on {channel}: {e}");
                return;
            }
        };
        let who = mac(&phone);
        if !sdp::offered() {
            println!("[accessory] {who} opened {name}, which no host takes");
            continue;
        }
        println!("[accessory] {who} opened {name} on channel {channel}");
        hand_over(link, &who, name, local, host);
    }
}

fn attend(host: &Mutex<Option<TcpStream>>) {
    let listener = match TcpListener::bind(("0.0.0.0", IAP)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[accessory] bind :{IAP}: {e}");
            return;
        }
    };
    for stream in livi_net::bridge::from_usb(&listener) {
        let _ = stream.set_nodelay(true);
        println!("[accessory] a host is ready for the next session on :{IAP}");
        *host.lock().unwrap() = Some(stream);
    }
}

fn rfcomm(channel: u8) -> Result<OwnedFd, String> {
    let raw = unsafe {
        libc::socket(AF_BLUETOOTH, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, BTPROTO_RFCOMM)
    };
    if raw < 0 {
        return Err(format!("rfcomm socket: {}", std::io::Error::last_os_error()));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let addr = SockaddrRc { family: AF_BLUETOOTH as libc::sa_family_t, bdaddr: [0; 6], channel };
    let bound = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &raw const addr as *const libc::sockaddr,
            size_of::<SockaddrRc>() as libc::socklen_t,
        )
    };
    if bound < 0 {
        return Err(format!("rfcomm bind {channel}: {}", std::io::Error::last_os_error()));
    }
    if unsafe { libc::listen(fd.as_raw_fd(), 2) } < 0 {
        return Err(format!("rfcomm listen: {}", std::io::Error::last_os_error()));
    }
    Ok(fd)
}

fn carry(link: std::fs::File, tcp: TcpStream) -> usize {
    let (Ok(mut out), Ok(mut back), Ok(down)) =
        (tcp.try_clone(), link.try_clone(), tcp.try_clone())
    else {
        return 0;
    };
    let raw = link.as_raw_fd();
    let host_to_phone = std::thread::spawn(move || {
        let mut input = down;
        let mut buf = [0u8; 2048];
        while let Ok(n) = input.read(&mut buf) {
            if n == 0 || back.write_all(&buf[..n]).is_err() {
                break;
            }
        }
        unsafe { libc::shutdown(raw, libc::SHUT_RDWR) };
    });
    let mut from_phone = &link;
    let mut buf = [0u8; 2048];
    let mut total = 0usize;
    while let Ok(n) = from_phone.read(&mut buf) {
        if n == 0 || out.write_all(&buf[..n]).is_err() {
            break;
        }
        total += n;
    }
    let _ = tcp.shutdown(std::net::Shutdown::Both);
    let _ = host_to_phone.join();
    total
}

/// The kernel takes the UUID in reversed byte order.
fn advertised(uuid: &[u8; 16]) -> Vec<u8> {
    let mut out: Vec<u8> = uuid.iter().rev().copied().collect();
    out.push(SERVICE_AUDIO);
    out
}

/// The fixed-width name field the kernel expects: 249 bytes, then 11 for the short name.
fn local_name(name: &str) -> Vec<u8> {
    let mut out = vec![0u8; 260];
    let bytes = name.as_bytes();
    let long = bytes.len().min(248);
    out[..long].copy_from_slice(&bytes[..long]);
    let short = bytes.len().min(10);
    out[249..249 + short].copy_from_slice(&bytes[..short]);
    out
}

fn listen(mgmt: &Mgmt) {
    loop {
        let (event, _, body) = match mgmt.event() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[accessory] {e}");
                return;
            }
        };
        match event {
            EV_USER_CONFIRM_REQUEST => {
                println!("[accessory] {} wants to bond, confirming", addr(&body));
                if let Err(e) = mgmt.call(USER_CONFIRM_REPLY, mgmt::INDEX, &body[..7]) {
                    eprintln!("[accessory] {e}");
                }
            }
            EV_PIN_CODE_REQUEST => {
                println!("[accessory] {} asked for a pin, turning it down", addr(&body));
                let _ = mgmt.call(PIN_CODE_NEG_REPLY, mgmt::INDEX, &body[..7]);
            }
            EV_NEW_LINK_KEY => {
                // The key sits behind a one byte store hint.
                println!("[accessory] bonded with {}", addr(&body[1..]));
                if body.len() > KEY_LEN {
                    remember(&body[1..1 + KEY_LEN]);
                }
            }
            EV_DEVICE_CONNECTED => println!("[accessory] {} connected", addr(&body)),
            EV_DEVICE_DISCONNECTED => println!("[accessory] {} disconnected", addr(&body)),
            EV_CONNECT_FAILED => println!("[accessory] {} would not connect", addr(&body)),
            EV_AUTH_FAILED => println!("[accessory] {} failed to authenticate", addr(&body)),
            EV_NEW_SETTINGS => {
                if body.len() >= 4 {
                    let bits = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
                    println!("[accessory] now {}", mgmt::settings(bits));
                }
            }
            other => println!("[accessory] event {other:#06x}"),
        }
    }
}

fn hand_over(
    link: std::fs::File,
    who: &str,
    name: &str,
    local: &str,
    host: &Mutex<Option<TcpStream>>,
) {
    let Some(mut tcp) = host.lock().unwrap().take() else {
        println!("[accessory] {name} for {who} with no host attached");
        return;
    };
    let head = format!("peer {who}\nlocal {local}\n\n");
    if tcp.write_all(head.as_bytes()).is_err() {
        return;
    }
    blue(true);
    let carried = carry(link, tcp);
    blue(false);
    println!("[accessory] {name} closed after {carried} bytes");
}

fn drop_phone(phone: &[u8; 6]) -> Result<(), String> {
    let mgmt = Mgmt::open()?;
    let mut who = phone.to_vec();
    who.push(0);
    mgmt.call(DISCONNECT, mgmt::INDEX, &who)?;
    Ok(())
}

/// Hosts give their orders through wifid's port, so this one stays on the device.
fn control() {
    let listener = match TcpListener::bind(("127.0.0.1", ACCESSORY)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[accessory] bind :{ACCESSORY}: {e}");
            return;
        }
    };
    println!("[accessory] taking orders on :{ACCESSORY}");
    for stream in listener.incoming().flatten() {
        let Ok(mut out) = stream.try_clone() else {
            continue;
        };
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let answer = match order(line.trim()) {
                Ok(text) => text,
                Err(e) => format!("error {e}\n"),
            };
            if out.write_all(answer.as_bytes()).is_err() {
                break;
            }
        }
    }
}

fn order(line: &str) -> Result<String, String> {
    match line {
        // Sets the controller up again after a host had it.
        "on" => {
            let mgmt = Mgmt::open()?;
            let name = wifid_ap_name().unwrap_or_else(|| NAME.into());
            present(&mgmt, &name)?;
            restore(&mgmt);
            advertise(true)?;
            Ok("ok\n".into())
        }
        "off" => {
            if let Err(e) = advertise(false) {
                println!("[accessory] off: {e}");
            }
            // A host that holds the controller keeps it off the air itself.
            if let Err(e) = offer(false) {
                println!("[accessory] off: {e}");
            }
            Ok("ok\n".into())
        }
        _ if line.starts_with("disconnect ") => {
            let phone = address(line.trim_start_matches("disconnect ")).ok_or("not an address")?;
            drop_phone(&phone).map(|()| "ok\n".into())
        }
        "status" => {
            let on = if sdp::offered() { "on" } else { "off" };
            Ok(format!("bonds {}\noffered {on}\nok\n", stored_keys().len()))
        }
        other => Err(format!("unknown order {other:?}")),
    }
}

/// Lists CarPlay's records and UUIDs, or takes them back.
fn advertise(on: bool) -> Result<(), String> {
    if !sdp::set_offered(on) {
        return Ok(());
    }
    let done = Mgmt::open().and_then(|mgmt| {
        for uuid in &sdp::ADVERTISED {
            let uuid = advertised(uuid);
            if on {
                mgmt.call(ADD_UUID, mgmt::INDEX, &uuid)?;
            } else {
                // The kernel takes the UUID alone, without the service hint.
                mgmt.call(REMOVE_UUID, mgmt::INDEX, &uuid[..16])?;
            }
        }
        Ok(())
    });
    if let Err(e) = done {
        sdp::set_offered(!on);
        return Err(e);
    }
    for record in &sdp::RECORDS {
        let what = if on { "offering" } else { "withdrew" };
        println!("[accessory] {what} {} on channel {}", record.name, record.channel);
    }
    Ok(())
}

fn offer(on: bool) -> Result<(), String> {
    let mgmt = Mgmt::open()?;
    let mut visible = vec![u8::from(on)];
    visible.extend_from_slice(&0u16.to_le_bytes());
    mgmt.call(SET_CONNECTABLE, mgmt::INDEX, &[u8::from(on)])?;
    // Not connectable is not discoverable either, and the kernel refuses to be told so.
    if on {
        mgmt.call(SET_DISCOVERABLE, mgmt::INDEX, &visible)?;
    }
    Ok(())
}

fn stored_keys() -> Vec<Vec<u8>> {
    let Ok(text) = std::fs::read_to_string(keys_path()) else {
        return Vec::new();
    };
    text.lines().filter_map(unhex).collect()
}

/// A bond kept with more after its key still restores, the rest is left out.
fn unhex(line: &str) -> Option<Vec<u8>> {
    let line = line.trim();
    if line.len() < KEY_LEN * 2 {
        return None;
    }
    (0..KEY_LEN).map(|i| u8::from_str_radix(line.get(i * 2..i * 2 + 2)?, 16).ok()).collect()
}

fn keep(records: Vec<Vec<u8>>) {
    let text: String = records
        .iter()
        .map(|r| r.iter().map(|b| format!("{b:02x}")).collect::<String>() + "\n")
        .collect();
    let path = keys_path();
    let temp = format!("{path}.new");
    if std::fs::write(&temp, text).is_ok() && std::fs::rename(&temp, &path).is_ok() {
        println!("[accessory] {} bonds kept", records.len());
        let _ = std::process::Command::new("/usr/bin/livid")
            .arg("config")
            .arg("save")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

fn remember(key: &[u8]) {
    if key.len() != KEY_LEN {
        return;
    }
    let mut records: Vec<Vec<u8>> =
        stored_keys().into_iter().filter(|r| r[..6] != key[..6]).collect();
    records.push(key.to_vec());
    keep(records);
}

fn restore(mgmt: &Mgmt) {
    let keys = stored_keys();
    if keys.is_empty() {
        return;
    }
    let mut params = vec![0u8];
    params.extend_from_slice(&(keys.len() as u16).to_le_bytes());
    for key in &keys {
        params.extend_from_slice(key);
    }
    match mgmt.call(LOAD_LINK_KEYS, mgmt::INDEX, &params) {
        Ok(_) => println!("[accessory] {} phones are still paired", keys.len()),
        Err(e) => eprintln!("[accessory] bonds not restored: {e}"),
    }
}

fn blue(on: bool) {
    if on {
        let _ = std::fs::create_dir_all("/tmp/livi/led");
        let _ = std::fs::write("/tmp/livi/led/bt-connected", "");
    } else {
        let _ = std::fs::remove_file("/tmp/livi/led/bt-connected");
    }
}

/// Reversed, the way the kernel stores an address.
fn address(text: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut parts = text.trim().split(':');
    for byte in out.iter_mut().rev() {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

fn mac(bdaddr: &[u8; 6]) -> String {
    bdaddr.iter().rev().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

fn addr(body: &[u8]) -> String {
    if body.len() < 6 {
        return "?".into();
    }
    let mut bdaddr = [0u8; 6];
    bdaddr.copy_from_slice(&body[..6]);
    mac(&bdaddr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_reads_back_the_way_it_is_written() {
        let phone = address("0C:6A:C4:4E:F3:2A").unwrap();
        assert_eq!(phone, [0x2a, 0xf3, 0x4e, 0xc4, 0x6a, 0x0c]);
        assert_eq!(mac(&phone), "0C:6A:C4:4E:F3:2A");
        assert_eq!(address("0C:6A:C4"), None);
    }

    #[test]
    fn a_bond_restores_from_its_key_whatever_follows_it() {
        let key = "2af34ec46a0c0004".to_string() + &"11".repeat(16) + "10";
        assert_eq!(unhex(&key).map(|k| k.len()), Some(KEY_LEN));
        assert_eq!(unhex(&(key.clone() + "04")), unhex(&key));
        assert_eq!(unhex(&key[..10]), None);
    }

    #[test]
    fn only_known_orders_are_taken() {
        assert!(order("hands-free on").is_err());
        assert!(order("targets 0C:6A:C4:4E:F3:2A").is_err());
        assert!(order("disconnect nonsense").is_err());
    }
}
