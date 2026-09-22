//! The dongle's Bluetooth controller, attached to this machine's own stack. `btd` on the dongle
//! serves the controller's HCI packets over TCP, `/dev/vhci` feeds them to BlueZ, and BlueZ then
//! carries the adapter like any other one.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::Mutex;
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
    notice_loss(&stream);
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

/// An unplugged dongle sends no goodbye, so a silent or unanswered tunnel ends after 3 s.
fn notice_loss(stream: &TcpStream) {
    for (level, name, value) in [
        (libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 2),
        (libc::IPPROTO_TCP, libc::TCP_USER_TIMEOUT, 3000),
    ] {
        let value: libc::c_int = value;
        let rc = unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                level,
                name,
                (&raw const value).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            eprintln!("[bt] tunnel timeout: {}", std::io::Error::last_os_error());
        }
    }
}

/// Copies HCI packets between the local stack and the dongle until either end closes. Each packet
/// gets a length in front of it, because TCP would otherwise hand the far side two at once.
fn pump(dev: File, stream: TcpStream, named: &(impl Fn(u16) + Sync)) {
    let (Ok(out), Ok(reader)) = (stream.try_clone(), dev.try_clone()) else {
        eprintln!("[bt] cannot split the tunnel");
        return;
    };
    let stop = AtomicBool::new(false);
    let out = Mutex::new(out);
    std::thread::scope(|threads| {
        threads.spawn(|| out_bound(reader, &out, &stop, named));
        in_bound(dev, &stream, &out);
        stop.store(true, Ordering::Relaxed);
        let _ = stream.shutdown(std::net::Shutdown::Both);
    });
}

/// Local stack to dongle. The read waits in slices so the tunnel can end while the device is quiet.
fn out_bound(dev: File, out: &Mutex<TcpStream>, stop: &AtomicBool, named: &impl Fn(u16)) {
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
        if let Err(e) = send(out, &buf[..n]) {
            eprintln!("[bt] send: {e}");
            return;
        }
    }
}

/// Both directions may have a packet for the dongle, so a frame goes out whole.
fn send(out: &Mutex<TcpStream>, pkt: &[u8]) -> std::io::Result<()> {
    let mut frame = Vec::with_capacity(2 + pkt.len());
    frame.extend_from_slice(&(pkt.len() as u16).to_be_bytes());
    frame.extend_from_slice(pkt);
    out.lock().unwrap().write_all(&frame)
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
fn in_bound(mut dev: File, stream: &TcpStream, out: &Mutex<TcpStream>) {
    let mut input = stream;
    let mut len = [0u8; 2];
    let mut buf = [0u8; PACKET_MAX];
    let mut own_orders = 0u32;
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
        // The local stack never asked for it.
        if own_orders > 0 && answers(&buf[..n], WRITE_VOICE_SETTING) {
            own_orders -= 1;
            continue;
        }
        if let Some(order) = fit_to_vhci(&mut buf[..n]) {
            if let Err(e) = send(out, &order) {
                eprintln!("[bt] send: {e}");
                return;
            }
            own_orders += 1;
        }
        if let Err(e) = dev.write_all(&buf[..n]) {
            eprintln!("[bt] write /dev/vhci: {e}");
            return;
        }
    }
}

const EVENT: u8 = 0x04;
const COMMAND: u8 = 0x01;
const COMMAND_COMPLETE: u8 = 0x0e;
const READ_LOCAL_COMMANDS: u16 = 0x1002;
const READ_VOICE_SETTING: u16 = 0x0c25;
const WRITE_VOICE_SETTING: u16 = 0x0c26;
const SYNC_FLOWCTL: (usize, u8) = (7 + 10, 1 << 4);
const VOICE_16BIT: u16 = 0x0060;

fn answers(pkt: &[u8], opcode: u16) -> bool {
    pkt.len() >= 7
        && pkt[0] == EVENT
        && pkt[1] == COMMAND_COMPLETE
        && u16::from_le_bytes([pkt[4], pkt[5]]) == opcode
}

/// Returns the order that makes the controller agree with what was corrected.
fn fit_to_vhci(pkt: &mut [u8]) -> Option<Vec<u8>> {
    if pkt.len() < 9 || pkt[6] != 0 {
        return None;
    }
    // Of all drivers only vhci lets the kernel switch call audio to flow control, and this
    // controller does not keep the count the kernel then waits for: the uplink runs dry.
    if answers(pkt, READ_LOCAL_COMMANDS) && pkt.len() > SYNC_FLOWCTL.0 {
        pkt[SYNC_FLOWCTL.0] &= !SYNC_FLOWCTL.1;
        return None;
    }
    // Linux only ever reads the setting, and this controller starts out at 8 bit.
    if answers(pkt, READ_VOICE_SETTING) && pkt[7..9] != VOICE_16BIT.to_le_bytes() {
        pkt[7..9].copy_from_slice(&VOICE_16BIT.to_le_bytes());
        let mut order = vec![COMMAND];
        order.extend_from_slice(&WRITE_VOICE_SETTING.to_le_bytes());
        order.push(2);
        order.extend_from_slice(&VOICE_16BIT.to_le_bytes());
        return Some(order);
    }
    None
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

#[cfg(test)]
mod tests {
    use super::{answers, fit_to_vhci};

    fn complete(opcode: u16, params: &[u8]) -> Vec<u8> {
        let mut pkt = vec![0x04, 0x0e, (4 + params.len()) as u8, 5];
        pkt.extend_from_slice(&opcode.to_le_bytes());
        pkt.push(0);
        pkt.extend_from_slice(params);
        pkt
    }

    #[test]
    fn call_audio_flow_control_is_not_offered() {
        let mut pkt = complete(0x1002, &[0xff; 64]);
        assert_eq!(fit_to_vhci(&mut pkt), None);
        assert_eq!(pkt[7 + 10], 0xef);
        assert!(pkt[7..].iter().enumerate().all(|(i, b)| i == 10 || *b == 0xff));
    }

    #[test]
    fn an_8_bit_controller_is_set_to_16_bit_and_reads_so() {
        let mut pkt = complete(0x0c25, &[0x00, 0x00]);
        let order = fit_to_vhci(&mut pkt);
        assert_eq!(&pkt[7..9], &[0x60, 0x00]);
        assert_eq!(order, Some(vec![0x01, 0x26, 0x0c, 0x02, 0x60, 0x00]));
    }

    #[test]
    fn a_16_bit_controller_is_left_alone() {
        let mut pkt = complete(0x0c25, &[0x60, 0x00]);
        let was = pkt.clone();
        assert_eq!(fit_to_vhci(&mut pkt), None);
        assert_eq!(pkt, was);
    }

    #[test]
    fn a_failed_or_other_answer_is_left_alone() {
        let mut failed = complete(0x0c25, &[0x00, 0x00]);
        failed[6] = 0x0c;
        let mut other = complete(0x1005, &[0xfd, 0x03, 0xff, 0x04, 0x00, 0x04, 0x00]);
        let (was_failed, was_other) = (failed.clone(), other.clone());
        assert_eq!(fit_to_vhci(&mut failed), None);
        assert_eq!(fit_to_vhci(&mut other), None);
        assert_eq!(failed, was_failed);
        assert_eq!(other, was_other);
    }

    #[test]
    fn the_answer_to_our_own_order_is_recognised() {
        assert!(answers(&complete(0x0c26, &[]), 0x0c26));
        assert!(!answers(&complete(0x0c25, &[0, 0]), 0x0c26));
        assert!(!answers(&[0x02, 0x0e, 0x04, 5, 0x26, 0x0c, 0], 0x0c26));
    }
}
