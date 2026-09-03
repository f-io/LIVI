// Android Auto over USB. A phone on the bus is switched to accessory mode (AOAP),
// its bulk pipe carries the session, and the main process hears about it the way
// it hears about a TCP one.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use nusb::descriptors::{ConfigurationDescriptor, TransferType};
use nusb::hotplug::HotplugEvent;
use nusb::transfer::{
    Buffer, Bulk, ControlIn, ControlOut, ControlType, Direction, In, Out, Recipient, TransferError,
};
use nusb::{Device, DeviceId, DeviceInfo, Endpoint, Interface};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use crate::session::{self, Peer};

const GOOGLE_VID: u16 = 0x18d1;
const ACCESSORY_PIDS: [u16; 6] = [0x2d00, 0x2d01, 0x2d02, 0x2d03, 0x2d04, 0x2d05];
/// Android vendors whose devices are probed for AOAP.
const PHONE_VENDORS: [u16; 16] = [
    0x0489, 0x04dd, 0x04e8, 0x0b05, 0x0bb4, 0x0e8d, 0x0fce, 0x1004, 0x109b, 0x12d1, 0x17ef,
    0x18d1, 0x19d2, 0x22b8, 0x2717, 0x2a70,
];
/// A class-0 device made only of these interface classes is not a phone.
const NON_PHONE_INTERFACE_CLASSES: [u8; 10] =
    [0x01, 0x02, 0x03, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0e, 0xe0];
/// Wireless controllers share vendor ids with phones (MediaTek), no phone carries one.
const WIRELESS_CONTROLLER_CLASS: u8 = 0xe0;

const REQ_GET_PROTOCOL: u8 = 51;
const REQ_SEND_STRING: u8 = 52;
const REQ_START: u8 = 53;
/// SEND_STRING index and text. Manufacturer and model are what the phone matches on.
const AOAP_STRINGS: [(u16, &str); 6] = [
    (0, "Android"),
    (1, "Android Auto"),
    (2, "LIVI Wired Android Auto host"),
    (3, "2.0.1"),
    (4, "https://github.com/f-io/LIVI"),
    (5, "LIVI-0001"),
];

const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
const CLAIM_RETRIES: usize = 5;
const CLAIM_RETRY: Duration = Duration::from_millis(100);
const WATCH_RETRY: Duration = Duration::from_secs(5);
/// The main process subscribes to the announcements right after starting the helper.
/// A phone already on the bus waits for that, or its session would find nobody.
const STARTUP_SCAN_DELAY: Duration = Duration::from_secs(3);
const READ_LEN: usize = 16384;
const READS_IN_FLIGHT: usize = 4;
/// The reset out of accessory mode re-enumerates the phone within this time.
const RESET_WINDOW: Duration = Duration::from_secs(3);

/// Tells the main process about a session: socket path, peer label, USB serial.
pub type OnSession = dyn Fn(&str, &str, &str) + Send + Sync;

#[derive(Default)]
struct Phones {
    /// Devices being switched or serving a session.
    busy: HashSet<DeviceId>,
    /// Devices whose AOAP probe failed, left alone until they re-enumerate.
    failed: HashSet<DeviceId>,
    serial_of: HashMap<DeviceId, String>,
    /// Serials the main process closed, not switched again until unplugged.
    parked: HashSet<String>,
    /// Serials being reset out of accessory mode, whose next disconnect is no unplug.
    resetting: HashMap<String, Instant>,
}

type Shared = Arc<Mutex<Phones>>;

pub async fn run(on_session: impl Fn(&str, &str, &str) + Send + Sync + 'static) {
    let on_session: Arc<OnSession> = Arc::new(on_session);
    let phones: Shared = Arc::default();
    let mut watch = loop {
        match nusb::watch_devices() {
            Ok(w) => break w,
            Err(e) => {
                eprintln!("[aa-usb] hotplug watch: {e}, retrying");
                tokio::time::sleep(WATCH_RETRY).await;
            }
        }
    };
    tokio::time::sleep(STARTUP_SCAN_DELAY).await;
    match nusb::list_devices().await {
        Ok(list) => {
            for info in list {
                seen(&phones, &on_session, info);
            }
        }
        Err(e) => eprintln!("[aa-usb] list devices: {e}"),
    }
    println!("[aa-usb] watching for phones");
    while let Some(ev) = watch.next().await {
        match ev {
            HotplugEvent::Connected(info) => seen(&phones, &on_session, info),
            HotplugEvent::Disconnected(id) => gone(&phones, id),
        }
    }
    eprintln!("[aa-usb] hotplug watch ended");
}

fn label(serial: &str) -> String {
    if serial.is_empty() { "usb:phone".to_owned() } else { format!("usb:{serial}") }
}

fn is_accessory(info: &DeviceInfo) -> bool {
    info.vendor_id() == GOOGLE_VID && ACCESSORY_PIDS.contains(&info.product_id())
}

/// A phone candidate: an Android vendor's class-0 or vendor-class device that is not
/// made only of non-phone interfaces and carries no wireless controller.
fn is_candidate(info: &DeviceInfo) -> bool {
    if !PHONE_VENDORS.contains(&info.vendor_id()) {
        return false;
    }
    if info.class() != 0x00 && info.class() != 0xff {
        return false;
    }
    let interfaces: Vec<u8> = info.interfaces().map(|i| i.class()).collect();
    if interfaces.contains(&WIRELESS_CONTROLLER_CLASS) {
        return false;
    }
    interfaces.is_empty() || !interfaces.iter().all(|c| NON_PHONE_INTERFACE_CLASSES.contains(c))
}

fn seen(phones: &Shared, on_session: &Arc<OnSession>, info: DeviceInfo) {
    let id = info.id();
    let serial = info.serial_number().unwrap_or("").to_owned();
    let accessory = is_accessory(&info);
    let candidate = !accessory && is_candidate(&info);
    if !accessory && !candidate {
        return;
    }
    {
        let mut p = phones.lock().unwrap();
        if p.busy.contains(&id) || p.failed.contains(&id) {
            return;
        }
        if !serial.is_empty() {
            p.serial_of.insert(id, serial.clone());
        }
        if candidate && p.parked.contains(&serial) {
            println!("[aa-usb] {}: parked since the main process closed it", label(&serial));
            return;
        }
        p.busy.insert(id);
    }
    let phones = phones.clone();
    if accessory {
        let on_session = on_session.clone();
        tokio::spawn(async move {
            serve(&phones, &on_session, info, serial).await;
            phones.lock().unwrap().busy.remove(&id);
        });
    } else {
        tokio::spawn(async move {
            let l = label(&serial);
            println!(
                "[aa-usb] {l}: phone candidate {:04x}:{:04x}, switching to accessory mode",
                info.vendor_id(),
                info.product_id()
            );
            match switch(&info).await {
                Ok(()) => println!("[aa-usb] {l}: accessory mode requested"),
                Err(e) => {
                    eprintln!("[aa-usb] {l}: aoap: {e}");
                    phones.lock().unwrap().failed.insert(id);
                }
            }
            phones.lock().unwrap().busy.remove(&id);
        });
    }
}

fn gone(phones: &Shared, id: DeviceId) {
    let mut p = phones.lock().unwrap();
    p.busy.remove(&id);
    p.failed.remove(&id);
    let Some(serial) = p.serial_of.remove(&id) else { return };
    // The reset out of accessory mode disconnects too. Anything later is the cable.
    if let Some(at) = p.resetting.remove(&serial)
        && at.elapsed() <= RESET_WINDOW
    {
        return;
    }
    if p.parked.remove(&serial) {
        println!("[aa-usb] {}: unplugged, no longer parked", label(&serial));
    }
}

/// The AOAP handshake: protocol probe, the host strings, then the switch.
async fn switch(info: &DeviceInfo) -> Result<(), String> {
    let dev = info.open().await.map_err(|e| format!("open: {e}"))?;
    let reply = dev
        .control_in(
            ControlIn {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: REQ_GET_PROTOCOL,
                value: 0,
                index: 0,
                length: 2,
            },
            CONTROL_TIMEOUT,
        )
        .await
        .map_err(|e| format!("get protocol: {e}"))?;
    if reply.len() < 2 {
        return Err("get protocol: short reply".into());
    }
    let version = u16::from_le_bytes([reply[0], reply[1]]);
    if version < 1 {
        return Err(format!("protocol {version} not supported"));
    }
    for (index, text) in AOAP_STRINGS {
        let mut data = text.as_bytes().to_vec();
        data.push(0);
        vendor_out(&dev, REQ_SEND_STRING, index, &data)
            .await
            .map_err(|e| format!("send string {index}: {e}"))?;
    }
    vendor_out(&dev, REQ_START, 0, &[]).await.map_err(|e| format!("start: {e}"))
}

async fn vendor_out(dev: &Device, request: u8, index: u16, data: &[u8]) -> Result<(), TransferError> {
    dev.control_out(
        ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request,
            value: 0,
            index,
            data,
        },
        CONTROL_TIMEOUT,
    )
    .await
}

/// Runs one session over the accessory, then puts the phone back into its plain mode.
async fn serve(phones: &Shared, on_session: &Arc<OnSession>, info: DeviceInfo, serial: String) {
    let l = label(&serial);
    let dev = match info.open().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[aa-usb] {l}: open accessory: {e}");
            return;
        }
    };
    let pipe = match open_pipe(&dev).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[aa-usb] {l}: {e}");
            return;
        }
    };
    let Some((node, path)) = crate::server::bind_session_socket("aa-session") else { return };
    println!("[aa-usb] {l}: accessory up, session socket {path}");
    on_session(&path, &l, &serial);
    let peer = Peer { label: l.clone(), ip: IpAddr::V4(Ipv4Addr::LOCALHOST) };
    let end = session::run(UsbStream::new(pipe), peer, node, path).await;
    if !serial.is_empty() {
        let mut p = phones.lock().unwrap();
        if end.closed_by_node {
            p.parked.insert(serial.clone());
        }
        p.resetting.insert(serial.clone(), Instant::now());
    }
    if let Err(e) = dev.reset().await {
        eprintln!("[aa-usb] {l}: reset: {e}");
    }
}

pub struct Pipe {
    iface: Interface,
    ep_in: Endpoint<Bulk, In>,
    ep_out: Endpoint<Bulk, Out>,
}

pub async fn open_pipe(dev: &Device) -> Result<Pipe, String> {
    let active = dev.active_configuration().map(|c| c.configuration_value()).ok();
    if active != Some(1) {
        dev.set_configuration(1).await.map_err(|e| format!("set configuration: {e}"))?;
    }
    let (number, in_addr, out_addr) = {
        let cfg = dev.active_configuration().map_err(|e| format!("configuration: {e}"))?;
        bulk_pair(&cfg).ok_or("no interface with bulk in and out")?
    };
    // Right after enumeration the OS may briefly refuse the claim.
    let mut last = String::new();
    for _ in 0..CLAIM_RETRIES {
        match dev.claim_interface(number).await {
            Ok(iface) => {
                let ep_in = iface.endpoint::<Bulk, In>(in_addr).map_err(|e| format!("bulk in: {e}"))?;
                let ep_out =
                    iface.endpoint::<Bulk, Out>(out_addr).map_err(|e| format!("bulk out: {e}"))?;
                return Ok(Pipe { iface, ep_in, ep_out });
            }
            Err(e) => {
                last = e.to_string();
                tokio::time::sleep(CLAIM_RETRY).await;
            }
        }
    }
    Err(format!("claim interface {number}: {last}"))
}

fn bulk_pair(cfg: &ConfigurationDescriptor<'_>) -> Option<(u8, u8, u8)> {
    for intf in cfg.interfaces() {
        let Some(alt) = intf.alt_settings().next() else { continue };
        let (mut ep_in, mut ep_out) = (None, None);
        for ep in alt.endpoints() {
            if ep.transfer_type() != TransferType::Bulk {
                continue;
            }
            match ep.direction() {
                Direction::In => ep_in = Some(ep.address()),
                Direction::Out => ep_out = Some(ep.address()),
            }
        }
        if let (Some(i), Some(o)) = (ep_in, ep_out) {
            return Some((intf.interface_number(), i, o));
        }
    }
    None
}

/// The bulk pipe as a byte stream for the session, pumped by two tasks.
pub struct UsbStream {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    leftover: Vec<u8>,
    eof: bool,
}

impl UsbStream {
    pub fn new(pipe: Pipe) -> Self {
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        tokio::spawn(pump_in(pipe.ep_in, in_tx, pipe.iface));
        tokio::spawn(pump_out(pipe.ep_out, out_rx));
        Self { rx: in_rx, tx: Some(out_tx), leftover: Vec::new(), eof: false }
    }
}

async fn pump_in(mut ep: Endpoint<Bulk, In>, tx: mpsc::UnboundedSender<Vec<u8>>, _iface: Interface) {
    for _ in 0..READS_IN_FLIGHT {
        let buf = ep.allocate(READ_LEN);
        ep.submit(buf);
    }
    loop {
        let done = ep.next_complete().await;
        match done.status {
            Ok(()) => {
                if done.actual_len > 0 && tx.send(done.buffer[..done.actual_len].to_vec()).is_err() {
                    break;
                }
            }
            Err(TransferError::Stall) => {
                if let Err(e) = ep.clear_halt().await {
                    eprintln!("[aa-usb] clear halt: {e}");
                    break;
                }
            }
            Err(e) => {
                eprintln!("[aa-usb] read: {e}");
                break;
            }
        }
        let buf = ep.allocate(READ_LEN);
        ep.submit(buf);
    }
}

async fn pump_out(mut ep: Endpoint<Bulk, Out>, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(chunk) = rx.recv().await {
        ep.submit(Buffer::from(chunk));
        let done = ep.next_complete().await;
        if let Err(e) = done.status {
            eprintln!("[aa-usb] write: {e}");
            break;
        }
    }
}

impl AsyncRead for UsbStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.leftover.is_empty() {
            if self.eof {
                return Poll::Ready(Ok(()));
            }
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(chunk)) => self.leftover = chunk,
                Poll::Ready(None) => {
                    self.eof = true;
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        let n = buf.remaining().min(self.leftover.len());
        buf.put_slice(&self.leftover[..n]);
        self.leftover.drain(..n);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for UsbStream {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let sent = self.tx.as_ref().is_some_and(|tx| tx.send(data.to_vec()).is_ok());
        if sent {
            Poll::Ready(Ok(data.len()))
        } else {
            Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.tx = None;
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn stream() -> (UsbStream, mpsc::UnboundedSender<Vec<u8>>, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        (UsbStream { rx: in_rx, tx: Some(out_tx), leftover: Vec::new(), eof: false }, in_tx, out_rx)
    }

    #[tokio::test]
    async fn reads_hand_out_a_chunk_across_short_reads_and_end_at_eof() {
        let (mut s, in_tx, _out) = stream();
        in_tx.send(vec![1, 2, 3, 4, 5]).unwrap();
        drop(in_tx);
        let mut buf = [0u8; 3];
        assert_eq!(s.read(&mut buf).await.unwrap(), 3);
        assert_eq!(buf, [1, 2, 3]);
        assert_eq!(s.read(&mut buf).await.unwrap(), 2);
        assert_eq!(&buf[..2], &[4, 5]);
        assert_eq!(s.read(&mut buf).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn writes_go_out_whole_and_fail_after_shutdown() {
        let (mut s, _in, mut out) = stream();
        s.write_all(&[9, 8, 7]).await.unwrap();
        assert_eq!(out.recv().await.unwrap(), vec![9, 8, 7]);
        s.shutdown().await.unwrap();
        assert!(s.write_all(&[1]).await.is_err());
        assert!(out.recv().await.is_none());
    }

    #[test]
    fn the_label_names_the_serial_or_falls_back() {
        assert_eq!(label("39181FDJH00276"), "usb:39181FDJH00276");
        assert_eq!(label(""), "usb:phone");
    }
}
