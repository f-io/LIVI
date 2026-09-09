//! The Bluetooth controller, handed to a host over TCP. `HCI_CHANNEL_USER` takes it from the
//! kernel's own stack, and the controller is down while a host holds it.

use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const PORT: u16 = 5002;

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_HCI: libc::c_int = 1;
const HCI_CHANNEL_USER: u16 = 1;
const DEV: u16 = 0;
const IFACE: &str = "hci0";
const PACKET_MAX: usize = 4096;
const POLL_MS: i32 = 200;
const ORDER_WAIT: std::time::Duration = std::time::Duration::from_secs(3);
const CONTROLLER_TRIES: u32 = 120;
const CONTROLLER_POLL: std::time::Duration = std::time::Duration::from_millis(500);

#[repr(C)]
struct SockaddrHci {
    family: libc::sa_family_t,
    dev: u16,
    channel: u16,
}

/// Reports whether the controller can be claimed, and gives it back.
pub fn probe() -> ExitCode {
    let _ = Command::new("hciconfig").args([IFACE, "down"]).status();
    let taken = claim();
    let held = taken.is_ok();
    drop(taken);
    let _ = Command::new("hciconfig").args([IFACE, "up"]).status();
    if held {
        println!("[bt] {IFACE} can be claimed exclusively, the tunnel is possible");
        return ExitCode::SUCCESS;
    }
    eprintln!("[bt] {IFACE} cannot be claimed");
    ExitCode::FAILURE
}

/// Serves the controller on TCP, one host at a time.
pub fn run() -> ExitCode {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[btd] bind :{PORT}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if !await_controller() {
        eprintln!("[btd] {IFACE} never appeared");
        return ExitCode::FAILURE;
    }
    println!("[btd] listening on :{PORT}");
    let _ = Command::new("hciconfig").args([IFACE, "up"]).status();
    for stream in listener.incoming().flatten() {
        let _ = stream.set_nodelay(true);
        tell_accessory("off");
        let _ = Command::new("hciconfig").args([IFACE, "down"]).status();
        match claim() {
            Ok(hci) => {
                println!("[btd] {IFACE} claimed, tunnelling");
                pump(File::from(hci), stream);
                println!("[btd] host gone, giving {IFACE} back");
            }
            Err(e) => eprintln!("[btd] {IFACE}: {e}"),
        }
        let _ = Command::new("hciconfig").args([IFACE, "up"]).status();
        tell_accessory("on");
    }
    ExitCode::SUCCESS
}

/// Sends one order to the accessory on this dongle.
fn tell_accessory(order: &str) {
    let Ok(mut control) = TcpStream::connect(("127.0.0.1", crate::iapd::CONTROL_PORT)) else {
        return;
    };
    let _ = control.write_all(format!("{order}\n").as_bytes());
    let mut answer = [0u8; 32];
    let _ = control.set_read_timeout(Some(ORDER_WAIT));
    let _ = (&control).read(&mut answer);
}

/// Waits for the controller to be attached.
pub fn await_controller() -> bool {
    let path = format!("/sys/class/bluetooth/{IFACE}");
    for _ in 0..CONTROLLER_TRIES {
        if std::path::Path::new(&path).exists() {
            return true;
        }
        std::thread::sleep(CONTROLLER_POLL);
    }
    false
}

/// Opens the controller for exclusive use.
fn claim() -> Result<OwnedFd, String> {
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
        dev: DEV,
        channel: HCI_CHANNEL_USER,
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
    Ok(fd)
}

/// Copies HCI packets between the controller and the host until either end closes. Every packet
/// carries a two byte length in front of it.
fn pump(dev: File, stream: TcpStream) {
    let (Ok(out), Ok(reader)) = (stream.try_clone(), dev.try_clone()) else {
        eprintln!("[btd] cannot split the tunnel");
        return;
    };
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let up = std::thread::spawn(move || out_bound(reader, out, &flag));
    in_bound(dev, &stream);
    stop.store(true, Ordering::Relaxed);
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let _ = up.join();
}

/// Controller to host, reading in slices.
fn out_bound(dev: File, mut out: TcpStream, stop: &AtomicBool) {
    let mut buf = [0u8; PACKET_MAX];
    while !stop.load(Ordering::Relaxed) {
        if !readable(dev.as_raw_fd(), POLL_MS) {
            continue;
        }
        let n = match (&dev).read(&mut buf) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) => {
                eprintln!("[btd] read hci0: {e}");
                return;
            }
        };
        if let Err(e) = out
            .write_all(&(n as u16).to_be_bytes())
            .and_then(|()| out.write_all(&buf[..n]))
        {
            eprintln!("[btd] send: {e}");
            return;
        }
    }
}

/// Host to controller. One framed packet in, one packet out.
fn in_bound(mut dev: File, stream: &TcpStream) {
    let mut input = stream;
    let mut len = [0u8; 2];
    let mut buf = [0u8; PACKET_MAX];
    loop {
        if let Err(e) = input.read_exact(&mut len) {
            if e.kind() != std::io::ErrorKind::UnexpectedEof {
                eprintln!("[btd] receive: {e}");
            }
            return;
        }
        let n = u16::from_be_bytes(len) as usize;
        if n == 0 || n > PACKET_MAX {
            eprintln!("[btd] frame of {n} bytes, dropping the tunnel");
            return;
        }
        if let Err(e) = input.read_exact(&mut buf[..n]) {
            eprintln!("[btd] receive: {e}");
            return;
        }
        if let Err(e) = dev.write_all(&buf[..n]) {
            // A packet the controller refuses is dropped, the framing stays in step.
            if e.raw_os_error() != Some(libc::EINVAL) {
                eprintln!("[btd] write {IFACE}: {e}");
                return;
            }
            eprintln!("[btd] {IFACE} refused a packet of type {:#04x}", buf[0]);
        }
    }
}

/// Whether the descriptor has something to read, waiting at most `ms`.
fn readable(fd: RawFd, ms: i32) -> bool {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&raw mut p, 1, ms) > 0 }
}
