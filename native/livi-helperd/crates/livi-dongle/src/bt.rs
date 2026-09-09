//! The dongle's Bluetooth controller, attached to this machine's own stack. `btd` on the dongle
//! serves the controller's HCI packets over TCP, `/dev/vhci` feeds them to BlueZ, and BlueZ then
//! carries the adapter like any other one.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::link;

pub const PORT: u16 = 5002;
const VHCI: &str = "/dev/vhci";
/// Where the kernel lists what is radio blocked.
const RFKILL: &str = "/sys/class/rfkill";
/// Marks the driver's own messages rather than controller traffic.
const VENDOR_PKT: u8 = 0xff;
/// Vendor packet plus opcode 0, which asks the driver for a primary adapter.
const CREATE_PRIMARY: [u8; 2] = [VENDOR_PKT, 0x00];
/// Enough for the largest ACL packet the controller will hand over.
const PACKET_MAX: usize = 4096;
/// How long a quiet device is waited on before the tunnel is rechecked.
const POLL_MS: i32 = 200;
/// How long a lost dongle is left alone before the tunnel is tried again.
const RETRY: Duration = Duration::from_secs(5);

/// Keeps the dongle's controller attached.
pub fn attach(
    on_adapter: impl Fn(u16) + Send + Sync + 'static,
    on_lost: impl Fn() + Send + 'static,
) {
    std::thread::spawn(move || {
        loop {
            match tunnel(&on_adapter) {
                Ok(true) => on_lost(),
                Ok(false) => {}
                Err(e) => eprintln!("[bt] {e}"),
            }
            std::thread::sleep(RETRY);
        }
    });
}

/// Runs the tunnel until the dongle or the local stack lets go, saying whether an adapter existed.
pub fn tunnel(on_adapter: &(impl Fn(u16) + Sync)) -> Result<bool, String> {
    let stream = TcpStream::connect(link::addr(PORT)).map_err(|e| format!("dongle: {e}"))?;
    stream
        .set_nodelay(true)
        .map_err(|e| format!("nodelay: {e}"))?;
    let mut dev = OpenOptions::new()
        .read(true)
        .write(true)
        .open(VHCI)
        .map_err(|e| format!("{VHCI}: {e}"))?;
    dev.write_all(&CREATE_PRIMARY)
        .map_err(|e| format!("{VHCI}: no adapter: {e}"))?;
    let made = AtomicBool::new(false);
    pump(dev, stream, &|index| {
        made.store(true, Ordering::Relaxed);
        on_adapter(index);
    });
    println!("[bt] tunnel closed");
    Ok(made.load(Ordering::Relaxed))
}

/// Copies HCI packets between the local stack and the dongle until either end closes. Each packet
/// gets a length in front of it, because TCP would otherwise hand the far side two at once.
fn pump(dev: File, stream: TcpStream, named: &(impl Fn(u16) + Sync)) {
    let (Ok(out), Ok(reader)) = (stream.try_clone(), dev.try_clone()) else {
        eprintln!("[bt] cannot split the tunnel");
        return;
    };
    let stop = AtomicBool::new(false);
    std::thread::scope(|threads| {
        threads.spawn(|| out_bound(reader, out, &stop, named));
        in_bound(dev, &stream);
        stop.store(true, Ordering::Relaxed);
        let _ = stream.shutdown(std::net::Shutdown::Both);
    });
}

/// Local stack to dongle. The read waits in slices so the tunnel can end while the device is quiet.
fn out_bound(dev: File, mut out: TcpStream, stop: &AtomicBool, named: &impl Fn(u16)) {
    let mut buf = [0u8; PACKET_MAX];
    while !stop.load(Ordering::Relaxed) {
        if !readable(dev.as_raw_fd(), POLL_MS) {
            continue;
        }
        let n = match (&dev).read(&mut buf) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) => {
                eprintln!("[bt] read {VHCI}: {e}");
                return;
            }
        };
        // The driver's own note that the adapter exists, carrying its index. It is not HCI.
        if buf[0] == VENDOR_PKT {
            if n >= 4 {
                let index = u16::from_le_bytes([buf[2], buf[3]]);
                println!("[bt] the dongle's controller is hci{index}");
                unblock(index);
                named(index);
            }
            continue;
        }
        if let Err(e) = out
            .write_all(&(n as u16).to_be_bytes())
            .and_then(|()| out.write_all(&buf[..n]))
        {
            eprintln!("[bt] send: {e}");
            return;
        }
    }
}

/// Clears the soft block a fresh adapter comes up with, which BlueZ would otherwise refuse to power.
fn unblock(index: u16) {
    let want = format!("hci{index}");
    let Ok(entries) = std::fs::read_dir(RFKILL) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if std::fs::read_to_string(path.join("name")).is_ok_and(|n| n.trim() == want) {
            if let Err(e) = std::fs::write(path.join("soft"), b"0") {
                eprintln!("[bt] {want} stays blocked: {e}");
            }
            return;
        }
    }
}

/// Dongle to local stack. One framed packet in, one packet out.
fn in_bound(mut dev: File, stream: &TcpStream) {
    let mut input = stream;
    let mut len = [0u8; 2];
    let mut buf = [0u8; PACKET_MAX];
    loop {
        if let Err(e) = input.read_exact(&mut len) {
            if e.kind() != std::io::ErrorKind::UnexpectedEof {
                eprintln!("[bt] receive: {e}");
            }
            return;
        }
        let n = u16::from_be_bytes(len) as usize;
        if n == 0 || n > PACKET_MAX {
            eprintln!("[bt] frame of {n} bytes, dropping the tunnel");
            return;
        }
        if let Err(e) = input.read_exact(&mut buf[..n]) {
            eprintln!("[bt] receive: {e}");
            return;
        }
        if let Err(e) = dev.write_all(&buf[..n]) {
            eprintln!("[bt] write /dev/vhci: {e}");
            return;
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
