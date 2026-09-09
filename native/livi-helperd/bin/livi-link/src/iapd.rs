//! The dongle as the Bluetooth accessory. It configures the controller over the management
//! socket, answers the kernel's pairing questions and keeps the link keys.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::mgmt::{self, Mgmt};
use crate::sdp;
use crate::wifid;

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_L2CAP: libc::c_int = 0;
const BTPROTO_RFCOMM: libc::c_int = 3;
const SOCK_SEQPACKET: libc::c_int = 5;
/// Where the bonds are kept, on the flash.
const KEYS: &str = "/etc/livi-bt-keys";
/// One stored bond: the address, its kind, the key and its length.
const KEY_LEN: usize = 25;
/// A bond may carry one more byte: the channel that phone answers iAP on.
const WITH_CHANNEL: usize = KEY_LEN + 1;
const SDP_PSM: u16 = 1;
/// The blue light: a flash per call, lit while iAP runs over Bluetooth, dark at the handover.
const LED_BLUE: u8 = 9;
const PULSE: std::time::Duration = std::time::Duration::from_millis(120);
pub const CONTROL_PORT: u16 = 5005;
/// The calling cadence: quick tries first, then slow ones.
const RING_FAST: std::time::Duration = std::time::Duration::from_secs(1);
const RING_FAST_TRIES: u32 = 15;
const RING_SLOW: std::time::Duration = std::time::Duration::from_secs(30);
/// How long no phone is called after a session moved on.
const RING_GRACE: std::time::Duration = std::time::Duration::from_secs(30);
/// Where the host picks up the iAP session.
pub const PORT: u16 = 5004;

#[repr(C)]
struct SockaddrL2 {
    family: libc::sa_family_t,
    psm: u16,
    bdaddr: [u8; 6],
    cid: u16,
    bdaddr_type: u8,
}

#[repr(C)]
struct SockaddrRc {
    family: libc::sa_family_t,
    bdaddr: [u8; 6],
    channel: u8,
}

/// The name used while the access point has none.
const NAME: &str = "LIVI Link";
/// How often the access point's name is checked.
const NAME_POLL: std::time::Duration = std::time::Duration::from_secs(5);
const CONTROLLER_TRIES: u32 = 120;
const CONTROLLER_POLL: std::time::Duration = std::time::Duration::from_millis(500);
/// Audio/Video, car audio.
const CLASS_MAJOR: u8 = 0x04;
const CLASS_MINOR: u8 = 0x20;
/// The audio service bit.
const SERVICE_AUDIO: u8 = 0x20;
/// No display and no keypad.
const IO_NO_INPUT_NO_OUTPUT: u8 = 0x03;

const SET_POWERED: u16 = 0x0005;
const SET_DISCOVERABLE: u16 = 0x0006;
const SET_CONNECTABLE: u16 = 0x0007;
const SET_BONDABLE: u16 = 0x0009;
const SET_SSP: u16 = 0x000b;
const SET_CLASS: u16 = 0x000e;
const SET_NAME: u16 = 0x000f;
const ADD_UUID: u16 = 0x0010;
const SET_IO_CAPABILITY: u16 = 0x0018;
const LOAD_LINK_KEYS: u16 = 0x0012;
const DISCONNECT: u16 = 0x0014;
const USER_CONFIRM_REPLY: u16 = 0x001c;
const PIN_CODE_NEG_REPLY: u16 = 0x0017;

const EV_NEW_SETTINGS: u16 = 0x0006;
const EV_NEW_LINK_KEY: u16 = 0x0009;
const EV_DEVICE_CONNECTED: u16 = 0x000b;
const EV_DEVICE_DISCONNECTED: u16 = 0x000c;
const EV_CONNECT_FAILED: u16 = 0x000d;
const EV_PIN_CODE_REQUEST: u16 = 0x000e;
const EV_USER_CONFIRM_REQUEST: u16 = 0x000f;
const EV_AUTH_FAILED: u16 = 0x0011;

pub fn run(args: &[String]) -> ExitCode {
    let fixed = args.first().cloned();
    let name = fixed
        .clone()
        .or_else(wifid::ap_name)
        .unwrap_or_else(|| NAME.into());
    let name = name.as_str();
    let Some((mgmt, local)) = ready() else {
        eprintln!("[iapd] the controller never answered");
        return ExitCode::FAILURE;
    };
    // Retried until the controller takes it.
    let mut refused = String::new();
    for _ in 0..CONTROLLER_TRIES {
        match present(&mgmt, name) {
            Ok(()) => {
                refused.clear();
                break;
            }
            Err(e) => {
                refused = e;
                std::thread::sleep(CONTROLLER_POLL);
            }
        }
    }
    if !refused.is_empty() {
        eprintln!("[iapd] {refused}");
        return ExitCode::FAILURE;
    }
    restore(&mgmt);
    println!("[iapd] {name} is discoverable and pairable");
    let linked = Arc::new(AtomicBool::new(false));
    // Off until a host asks for it.
    let calling = Arc::new(AtomicBool::new(false));
    // The host waiting for the next session.
    let host: Arc<Mutex<Option<TcpStream>>> = Arc::default();
    // When a session last moved on.
    let served: Arc<Mutex<Option<std::time::Instant>>> = Arc::default();
    let (busy, want, calling_to, called, rung) = (
        linked.clone(),
        calling.clone(),
        host.clone(),
        local.clone(),
        served.clone(),
    );
    std::thread::spawn(move || ring(&busy, &want, &called, &calling_to, &rung));
    let want = calling.clone();
    std::thread::spawn(move || control(&want));
    if fixed.is_none() {
        let mut shown = name.to_string();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(NAME_POLL);
                let Some(current) = wifid::ap_name() else {
                    continue;
                };
                if current == shown {
                    continue;
                }
                match Mgmt::open().and_then(|m| {
                    m.call(SET_NAME, mgmt::INDEX, &local_name(&current))
                        .map(|_| ())
                }) {
                    Ok(()) => {
                        println!("[iapd] the car is now called {current}");
                        shown = current;
                    }
                    Err(e) => eprintln!("[iapd] renaming to {current}: {e}"),
                }
            }
        });
    }
    std::thread::spawn(|| {
        if let Err(e) = sdp::serve() {
            eprintln!("[sdp] {e}");
        }
    });
    std::thread::spawn(move || {
        if let Err(e) = channel(&local, host, served) {
            eprintln!("[iapd] {e}");
        }
    });
    listen(&mgmt, &linked);
    ExitCode::SUCCESS
}

/// The management socket and the controller's own address, once it answers.
fn ready() -> Option<(Mgmt, String)> {
    for _ in 0..CONTROLLER_TRIES {
        if let Ok(mgmt) = Mgmt::open()
            && let Ok(info) = mgmt
                .call(mgmt::READ_INFO, mgmt::INDEX, &[])
                .and_then(|b| mgmt::info(&b))
        {
            let local = mac(&info.address);
            return Some((mgmt, local));
        }
        std::thread::sleep(CONTROLLER_POLL);
    }
    None
}

/// Sets name, class, services, and makes the controller connectable and discoverable.
fn present(mgmt: &Mgmt, name: &str) -> Result<(), String> {
    mgmt.call(SET_POWERED, mgmt::INDEX, &[1])?;
    mgmt.call(SET_SSP, mgmt::INDEX, &[1])?;
    mgmt.call(SET_IO_CAPABILITY, mgmt::INDEX, &[IO_NO_INPUT_NO_OUTPUT])?;
    mgmt.call(SET_BONDABLE, mgmt::INDEX, &[1])?;
    mgmt.call(SET_CLASS, mgmt::INDEX, &[CLASS_MAJOR, CLASS_MINOR])?;
    for uuid in [&sdp::IAP_UUID, &sdp::IAP_CLIENT_UUID, &sdp::CARPLAY_UUID] {
        mgmt.call(ADD_UUID, mgmt::INDEX, &advertised(uuid))?;
    }
    mgmt.call(SET_NAME, mgmt::INDEX, &local_name(name))?;
    mgmt.call(SET_CONNECTABLE, mgmt::INDEX, &[1])?;
    // A timeout of zero stays visible.
    let mut visible = vec![1u8];
    visible.extend_from_slice(&0u16.to_le_bytes());
    mgmt.call(SET_DISCOVERABLE, mgmt::INDEX, &visible)?;
    Ok(())
}

/// Opens every channel the record offers and hands the one the phone takes to the host.
fn channel(
    local: &str,
    host: Arc<Mutex<Option<TcpStream>>>,
    served: Arc<Mutex<Option<std::time::Instant>>>,
) -> Result<(), String> {
    let waiting = host.clone();
    std::thread::spawn(move || attend(&waiting));
    let mut open = Vec::new();
    for record in &sdp::RECORDS {
        let listener = rfcomm(record.channel)?;
        let slot = host.clone();
        let (channel, name) = (record.channel, record.name);
        let local = local.to_string();
        let seen = served.clone();
        open.push(std::thread::spawn(move || {
            take(&listener, channel, name, &local, &slot, &seen)
        }));
    }
    println!("[iapd] waiting for a phone, host on :{PORT}");
    for thread in open {
        let _ = thread.join();
    }
    Ok(())
}

/// Accepts on one channel, session after session.
fn take(
    listener: &OwnedFd,
    channel: u8,
    name: &str,
    local: &str,
    host: &Mutex<Option<TcpStream>>,
    served: &Mutex<Option<std::time::Instant>>,
) {
    loop {
        let mut peer = SockaddrRc {
            family: 0,
            bdaddr: [0; 6],
            channel: 0,
        };
        let mut size = size_of::<SockaddrRc>() as libc::socklen_t;
        let raw = unsafe {
            libc::accept(
                listener.as_raw_fd(),
                &raw mut peer as *mut libc::sockaddr,
                &raw mut size,
            )
        };
        if raw < 0 {
            eprintln!(
                "[iapd] accept on {channel}: {}",
                std::io::Error::last_os_error()
            );
            return;
        }
        let link = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(raw) });
        let phone = mac(&peer.bdaddr);
        println!("[iapd] {phone} opened {name} on channel {channel}");
        hand_over(link, &phone, name, local, host, served);
    }
}

/// Holds the host that wants the next session, keeping only the newest.
fn attend(host: &Mutex<Option<TcpStream>>) {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[iapd] bind :{PORT}: {e}");
            return;
        }
    };
    for stream in listener.incoming().flatten() {
        let _ = stream.set_nodelay(true);
        println!("[iapd] a host is ready for the next session");
        *host.lock().unwrap() = Some(stream);
    }
}

/// Opens one channel for anyone to connect to.
fn rfcomm(channel: u8) -> Result<OwnedFd, String> {
    let raw = unsafe {
        libc::socket(
            AF_BLUETOOTH,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            BTPROTO_RFCOMM,
        )
    };
    if raw < 0 {
        return Err(format!(
            "rfcomm socket: {}",
            std::io::Error::last_os_error()
        ));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let addr = SockaddrRc {
        family: AF_BLUETOOTH as libc::sa_family_t,
        bdaddr: [0; 6],
        channel,
    };
    let bound = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &raw const addr as *const libc::sockaddr,
            size_of::<SockaddrRc>() as libc::socklen_t,
        )
    };
    if bound < 0 {
        return Err(format!(
            "rfcomm bind {channel}: {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { libc::listen(fd.as_raw_fd(), 2) } < 0 {
        return Err(format!(
            "rfcomm listen: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(fd)
}

/// Copies bytes both ways until either side is done, and says how many came from the phone.
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

/// One service for discovery, in the byte order the socket wants.
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

/// Answers the kernel's pairing questions and logs the events.
fn listen(mgmt: &Mgmt, linked: &AtomicBool) {
    loop {
        let (event, _, body) = match mgmt.event() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[iapd] {e}");
                return;
            }
        };
        match event {
            EV_USER_CONFIRM_REQUEST => {
                println!("[iapd] {} wants to bond, confirming", addr(&body));
                if let Err(e) = mgmt.call(USER_CONFIRM_REPLY, mgmt::INDEX, &body[..7]) {
                    eprintln!("[iapd] {e}");
                }
            }
            EV_PIN_CODE_REQUEST => {
                println!("[iapd] {} asked for a pin, turning it down", addr(&body));
                let _ = mgmt.call(PIN_CODE_NEG_REPLY, mgmt::INDEX, &body[..7]);
            }
            EV_NEW_LINK_KEY => {
                // The key sits behind a one byte store hint.
                println!("[iapd] bonded with {}", addr(&body[1..]));
                if body.len() > KEY_LEN {
                    remember(&body[1..1 + KEY_LEN]);
                }
            }
            EV_DEVICE_CONNECTED => {
                linked.store(true, Ordering::Relaxed);
                println!("[iapd] {} connected", addr(&body));
            }
            EV_DEVICE_DISCONNECTED => {
                linked.store(false, Ordering::Relaxed);
                println!("[iapd] {} disconnected", addr(&body));
            }
            EV_CONNECT_FAILED => println!("[iapd] {} would not connect", addr(&body)),
            EV_AUTH_FAILED => println!("[iapd] {} failed to authenticate", addr(&body)),
            EV_NEW_SETTINGS => {
                if body.len() >= 4 {
                    let bits = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
                    println!("[iapd] now {}", mgmt::settings(bits));
                }
            }
            other => println!("[iapd] event {other:#06x}"),
        }
    }
}

/// Gives one channel to the waiting host, peer and local address in front.
fn hand_over(
    link: std::fs::File,
    phone: &str,
    name: &str,
    local: &str,
    host: &Mutex<Option<TcpStream>>,
    served: &Mutex<Option<std::time::Instant>>,
) {
    let Some(mut tcp) = host.lock().unwrap().take() else {
        println!("[iapd] {name} for {phone} with no host attached");
        return;
    };
    // Peer, local, a blank line, then the session's own bytes.
    let head = format!("peer {phone}\nlocal {local}\n\n");
    if tcp.write_all(head.as_bytes()).is_err() {
        return;
    }
    blue(true);
    let carried = carry(link, tcp);
    blue(false);
    *served.lock().unwrap() = Some(std::time::Instant::now());
    println!("[iapd] {name} closed after {carried} bytes");
}

/// Calls the stored phones while none is connected and calling is on.
fn ring(
    linked: &AtomicBool,
    wanted: &AtomicBool,
    local: &str,
    host: &Mutex<Option<TcpStream>>,
    served: &Mutex<Option<std::time::Instant>>,
) {
    let mut tries: u32 = 0;
    loop {
        std::thread::sleep(if tries < RING_FAST_TRIES {
            RING_FAST
        } else {
            RING_SLOW
        });
        if linked.load(Ordering::Relaxed) || !wanted.load(Ordering::Relaxed) {
            tries = 0;
            continue;
        }
        if served
            .lock()
            .unwrap()
            .is_some_and(|when| when.elapsed() < RING_GRACE)
        {
            continue;
        }
        let keys = stored_keys();
        if keys.is_empty() {
            continue;
        }
        tries = tries.saturating_add(1);
        for key in &keys {
            if linked.load(Ordering::Relaxed) || !wanted.load(Ordering::Relaxed) {
                break;
            }
            let mut phone = [0u8; 6];
            phone.copy_from_slice(&key[..6]);
            let who = addr(&phone);
            println!("[iapd] calling {who}");
            pulse();
            if let Some(link) = call(&phone) {
                println!("[iapd] {who} answered");
                tries = 0;
                hand_over(link, &who, "Wireless iAP", local, host, served);
                break;
            }
        }
    }
}

/// The control port: whether the car is offered, and whether known phones are called.
fn control(wanted: &AtomicBool) {
    let listener = match TcpListener::bind(("0.0.0.0", CONTROL_PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[iapd] bind :{CONTROL_PORT}: {e}");
            return;
        }
    };
    println!("[iapd] taking orders on :{CONTROL_PORT}");
    for stream in listener.incoming().flatten() {
        let Ok(mut out) = stream.try_clone() else {
            continue;
        };
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let answer = match order(line.trim(), wanted) {
                Ok(text) => text,
                Err(e) => format!("error {e}\n"),
            };
            if out.write_all(answer.as_bytes()).is_err() {
                break;
            }
        }
    }
}

/// One order and its answer.
fn order(line: &str, wanted: &AtomicBool) -> Result<String, String> {
    match line {
        // Sets the controller up again after a host had it.
        "on" => {
            let mgmt = Mgmt::open()?;
            let name = wifid::ap_name().unwrap_or_else(|| NAME.into());
            present(&mgmt, &name)?;
            restore(&mgmt);
            Ok("ok\n".into())
        }
        "off" => {
            // Stops calling as well.
            wanted.store(false, Ordering::Relaxed);
            offer(false).map(|()| "ok\n".into())
        }
        "reconnect on" => {
            wanted.store(true, Ordering::Relaxed);
            Ok("ok\n".into())
        }
        "reconnect off" => {
            wanted.store(false, Ordering::Relaxed);
            Ok("ok\n".into())
        }
        _ if line.starts_with("disconnect ") => {
            let phone = address(line.trim_start_matches("disconnect ")).ok_or("not an address")?;
            let mgmt = Mgmt::open()?;
            let mut who = phone.to_vec();
            who.push(0);
            mgmt.call(DISCONNECT, mgmt::INDEX, &who)?;
            Ok("ok\n".into())
        }
        "status" => Ok(format!(
            "bonds {}\nreconnect {}\nok\n",
            stored_keys().len(),
            if wanted.load(Ordering::Relaxed) {
                "on"
            } else {
                "off"
            }
        )),
        other => Err(format!("unknown order {other:?}")),
    }
}

/// Connectable and discoverable, or neither.
fn offer(on: bool) -> Result<(), String> {
    let mgmt = Mgmt::open()?;
    let mut visible = vec![u8::from(on)];
    visible.extend_from_slice(&0u16.to_le_bytes());
    mgmt.call(SET_CONNECTABLE, mgmt::INDEX, &[u8::from(on)])?;
    mgmt.call(SET_DISCOVERABLE, mgmt::INDEX, &visible)?;
    Ok(())
}

/// Every bond, each one optionally carrying the channel learned for it.
fn stored_keys() -> Vec<Vec<u8>> {
    let Ok(text) = std::fs::read_to_string(KEYS) else {
        return Vec::new();
    };
    text.lines().filter_map(unhex).collect()
}

/// One stored line back into bytes.
fn unhex(line: &str) -> Option<Vec<u8>> {
    let line = line.trim();
    if line.len() != KEY_LEN * 2 && line.len() != WITH_CHANNEL * 2 {
        return None;
    }
    (0..line.len() / 2)
        .map(|i| u8::from_str_radix(line.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

/// Writes all bonds back.
fn keep(records: Vec<Vec<u8>>) {
    let text: String = records
        .iter()
        .map(|r| r.iter().map(|b| format!("{b:02x}")).collect::<String>() + "\n")
        .collect();
    let temp = format!("{KEYS}.new");
    if std::fs::write(&temp, text).is_ok() && std::fs::rename(&temp, KEYS).is_ok() {
        println!("[iapd] {} bonds kept", records.len());
    }
}

/// Keeps one bond, carrying over the channel we already learned for that phone.
fn remember(key: &[u8]) {
    if key.len() != KEY_LEN {
        return;
    }
    let old = stored_keys();
    let channel = old
        .iter()
        .find(|r| r[..6] == key[..6] && r.len() == WITH_CHANNEL)
        .map(|r| r[KEY_LEN]);
    let mut records: Vec<Vec<u8>> = old.into_iter().filter(|r| r[..6] != key[..6]).collect();
    let mut fresh = key.to_vec();
    if let Some(channel) = channel {
        fresh.push(channel);
    }
    records.push(fresh);
    keep(records);
}

/// Keeps the channel a phone answers iAP on.
fn remember_channel(phone: &[u8; 6], channel: u8) {
    let mut records = stored_keys();
    let Some(record) = records.iter_mut().find(|r| r[..6] == phone[..]) else {
        return;
    };
    if record.len() == WITH_CHANNEL {
        if record[KEY_LEN] == channel {
            return;
        }
        record[KEY_LEN] = channel;
    } else {
        record.push(channel);
    }
    println!("[iapd] {} answers iAP on channel {channel}", addr(phone));
    keep(records);
}

/// The channel learned for one phone.
fn channel_of(phone: &[u8; 6]) -> Option<u8> {
    stored_keys()
        .iter()
        .find(|r| r[..6] == phone[..] && r.len() == WITH_CHANNEL)
        .map(|r| r[KEY_LEN])
}

/// Hands the kernel the bonds from earlier runs.
fn restore(mgmt: &Mgmt) {
    let keys = stored_keys();
    if keys.is_empty() {
        return;
    }
    let mut params = vec![0u8];
    params.extend_from_slice(&(keys.len() as u16).to_le_bytes());
    for key in &keys {
        params.extend_from_slice(&key[..KEY_LEN]);
    }
    match mgmt.call(LOAD_LINK_KEYS, mgmt::INDEX, &params) {
        Ok(_) => println!("[iapd] {} phones are still paired", keys.len()),
        Err(e) => eprintln!("[iapd] bonds not restored: {e}"),
    }
}

/// Calls a phone we know: which channel carries its iAP service, then that channel.
fn call(phone: &[u8; 6]) -> Option<std::fs::File> {
    // A known channel is dialled straight away.
    if let Some(channel) = channel_of(phone) {
        return rfcomm_to(phone, channel);
    }
    let sdp = l2cap(phone, SDP_PSM)?;
    let channel = ask_channel(sdp)?;
    remember_channel(phone, channel);
    rfcomm_to(phone, channel)
}

/// Opens one L2CAP channel to a phone.
fn l2cap(phone: &[u8; 6], psm: u16) -> Option<std::fs::File> {
    let raw = unsafe { libc::socket(AF_BLUETOOTH, SOCK_SEQPACKET, BTPROTO_L2CAP) };
    if raw < 0 {
        return None;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let addr = SockaddrL2 {
        family: AF_BLUETOOTH as libc::sa_family_t,
        psm: psm.to_le(),
        bdaddr: *phone,
        cid: 0,
        bdaddr_type: 0,
    };
    let joined = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            &raw const addr as *const libc::sockaddr,
            size_of::<SockaddrL2>() as libc::socklen_t,
        )
    };
    (joined == 0).then(|| std::fs::File::from(fd))
}

/// Asks a phone where its iAP service lives. The channel sits right behind the RFCOMM marker.
fn ask_channel(mut sdp: std::fs::File) -> Option<u8> {
    let mut params = Vec::new();
    params.extend_from_slice(&sdp::seq(&sdp::uuid128(&sdp::IAP_CLIENT_UUID)));
    params.extend_from_slice(&u16::MAX.to_be_bytes());
    params.extend_from_slice(&sdp::seq(&sdp::uint16(0x0004)));
    params.push(0);
    sdp.write_all(&sdp::packet(0x06, 1, &params)).ok()?;
    let mut buf = [0u8; 1024];
    let n = sdp.read(&mut buf).ok()?;
    buf[..n]
        .windows(5)
        .find(|w| w[..4] == [0x19, 0x00, 0x03, 0x08])
        .map(|w| w[4])
}

/// Opens one channel on a phone.
fn rfcomm_to(phone: &[u8; 6], channel: u8) -> Option<std::fs::File> {
    let raw = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_STREAM, BTPROTO_RFCOMM) };
    if raw < 0 {
        return None;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let addr = SockaddrRc {
        family: AF_BLUETOOTH as libc::sa_family_t,
        bdaddr: *phone,
        channel,
    };
    let joined = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            &raw const addr as *const libc::sockaddr,
            size_of::<SockaddrRc>() as libc::socklen_t,
        )
    };
    (joined == 0).then(|| std::fs::File::from(fd))
}

/// A short blue flash.
fn pulse() {
    blue(true);
    std::thread::sleep(PULSE);
    blue(false);
}

/// The blue light, on or off. Zero lights it.
fn blue(on: bool) {
    let base = format!("/sys/class/gpio/gpio{LED_BLUE}");
    if !std::path::Path::new(&base).exists() {
        let _ = std::fs::write("/sys/class/gpio/export", LED_BLUE.to_string());
        let _ = std::fs::write(format!("{base}/direction"), "out");
    }
    let _ = std::fs::write(format!("{base}/value"), if on { "0" } else { "1" });
}

/// Six bytes out of a written address, in the order the kernel wants.
fn address(text: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut parts = text.trim().split(':');
    for byte in out.iter_mut().rev() {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// A controller address as text.
fn mac(bdaddr: &[u8; 6]) -> String {
    bdaddr
        .iter()
        .rev()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// The address at the front of most events, as text.
fn addr(body: &[u8]) -> String {
    if body.len() < 6 {
        return "?".into();
    }
    let mut bdaddr = [0u8; 6];
    bdaddr.copy_from_slice(&body[..6]);
    mac(&bdaddr)
}
