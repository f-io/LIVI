//! The host on top of the dongle's controller, reached through its HCI tunnel: links, pairing,
//! L2CAP, SDP, RFCOMM and call audio. Profiles get sockets, the way the kernel hands them out on
//! Linux.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::hci::{self, Addr, Event, text};
use crate::keys::Keys;
use crate::l2cap::{self, Signal};
use crate::rfcomm::{self, Happened, Session};
use crate::sdp::{self, Service};

const COMMAND_WAIT: Duration = Duration::from_secs(3);
/// A controller that never answered a command would stall every one after it.
const COMMAND_STALL: Duration = Duration::from_secs(2);
/// A page lasts up to 5.12 s, the rest is room for the controller.
const PAGE_WAIT: Duration = Duration::from_secs(10);
/// Pairing may wait on whoever holds the phone.
const SECURE_WAIT: Duration = Duration::from_secs(30);
const STEP_WAIT: Duration = Duration::from_secs(10);
const PACKET_MAX: usize = 4096;
/// NoInputNoOutput, no OOB data, general bonding without MITM protection.
const IO_CAPABILITY: [u8; 3] = [0x03, 0x00, 0x04];
/// Every event up to the low energy ones, the pairing ones included.
const EVENT_MASK: [u8; 8] = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xbf, 0x3d];
/// Audio/video, car audio, with the audio service bit.
const CLASS: [u8; 3] = [0x20, 0x04, 0x20];
/// CVSD on the air, 16-bit linear PCM over HCI. The controller comes up with 8-bit samples.
const VOICE_16BIT: u16 = 0x0060;
/// Role switch and sniff, as a phone parks an idle hands-free link in sniff.
const LINK_POLICY: u16 = 0x0005;
/// DM1, DH1, DM3, DH3, DM5 and DH5.
const ACL_PACKETS: u16 = 0xcc18;
/// Every SCO and eSCO packet type, the EDR ones included.
const SYNC_PACKETS: u16 = 0x003f;
const SYNC_BANDWIDTH: u32 = 8000;
const REMAIN_PERIPHERAL: u8 = 0x01;
const USER_ENDED: u8 = 0x13;
const NO_RESOURCES: u8 = 0x0d;
const SCAN_INQUIRY_AND_PAGE: u8 = 0x03;
/// The call socket between our pump and the profile.
const CALL_SEND_WAIT: Duration = Duration::from_millis(20);
const CALL_RECV_WAIT: Duration = Duration::from_millis(500);

pub struct Config {
    pub name: String,
    pub keys: PathBuf,
}

/// A phone opened one of our channels.
pub struct Incoming {
    pub channel: u8,
    pub peer: Addr,
    pub stream: UnixStream,
}

/// A call's audio, a datagram per packet both ways. An empty one means the call is over.
pub struct Call {
    pub peer: Addr,
    pub socket: UnixDatagram,
}

#[derive(Debug, PartialEq)]
pub enum Notice {
    Connected(Addr),
    Disconnected(Addr),
    Bonded(Addr),
    /// The tunnel closed, nothing works any more.
    Ended,
}

pub struct Inbox {
    pub incoming: mpsc::Receiver<Incoming>,
    pub calls: mpsc::Receiver<Call>,
    pub notices: mpsc::Receiver<Notice>,
}

struct Outlets {
    incoming: mpsc::Sender<Incoming>,
    calls: mpsc::Sender<Call>,
    notices: mpsc::Sender<Notice>,
}

#[derive(Clone)]
pub struct Stack {
    shared: Arc<Shared>,
}

struct Shared {
    tunnel: Mutex<TcpStream>,
    state: Mutex<State>,
    changed: Condvar,
    outlets: Mutex<Option<Outlets>>,
    /// The controller has one page train.
    paging: Mutex<()>,
}

struct Pipe {
    deliver: mpsc::Sender<Vec<u8>>,
    ours: UnixStream,
}

struct Mux {
    session: Session,
    pipes: HashMap<u8, Pipe>,
    /// Channels we asked for, and what became of them.
    wanted: HashSet<u8>,
    ready: HashMap<u8, UnixStream>,
    refused: HashSet<u8>,
}

impl Mux {
    fn new(session: Session) -> Self {
        Self {
            session,
            pipes: HashMap::new(),
            wanted: HashSet::new(),
            ready: HashMap::new(),
            refused: HashSet::new(),
        }
    }

    fn close(&mut self) {
        for (_, pipe) in self.pipes.drain() {
            let _ = pipe.ours.shutdown(Shutdown::Both);
        }
    }
}

struct Link {
    addr: Addr,
    in_flight: usize,
    joiner: l2cap::Joiner,
    l2cap: l2cap::Link,
    auth: Option<u8>,
    encryption: Option<u8>,
    encrypted: bool,
    opened: HashSet<u16>,
    refused: HashSet<u16>,
    sdp_replies: HashMap<u16, Vec<u8>>,
    muxes: HashMap<u16, Mux>,
}

impl Link {
    fn new(addr: Addr) -> Self {
        Self {
            addr,
            in_flight: 0,
            joiner: l2cap::Joiner::default(),
            l2cap: l2cap::Link::default(),
            auth: None,
            encryption: None,
            encrypted: false,
            opened: HashSet::new(),
            refused: HashSet::new(),
            sdp_replies: HashMap::new(),
            muxes: HashMap::new(),
        }
    }

    fn close(&mut self) {
        for mux in self.muxes.values_mut() {
            mux.close();
        }
    }
}

struct CallLink {
    socket: Arc<UnixDatagram>,
    seen: bool,
}

struct State {
    alive: bool,
    name: String,
    local: Addr,
    acl_mtu: usize,
    acl_free: usize,
    acl_max: usize,
    command_credits: u8,
    commands: VecDeque<Vec<u8>>,
    answers: HashMap<u16, Result<Vec<u8>, u8>>,
    acl: VecDeque<(u16, Vec<u8>)>,
    links: HashMap<u16, Link>,
    calls: HashMap<u16, CallLink>,
    pages: HashMap<Addr, Option<Result<u16, u8>>>,
    keys: Keys,
    offered: HashSet<Service>,
}

impl State {
    fn command(&mut self, opcode: u16, params: &[u8]) {
        self.commands.push_back(hci::command(opcode, params));
    }

    fn handle_of(&self, addr: &Addr) -> Option<u16> {
        self.links.iter().find(|(_, l)| l.addr == *addr).map(|(h, _)| *h)
    }

    fn push(&mut self, handle: u16, pdu: &[u8]) {
        for pkt in l2cap::split(handle, pdu, self.acl_mtu) {
            self.acl.push_back((handle, pkt));
        }
    }

    /// A payload for the channel we know as `local`.
    fn send_on(&mut self, handle: u16, local: u16, payload: &[u8]) {
        let remote = self.links.get(&handle).and_then(|l| l.l2cap.channel(local)).map(|c| c.remote);
        if let Some(remote) = remote {
            self.push(handle, &l2cap::pdu(remote, payload));
        }
    }

    fn mux(&mut self, handle: u16, cid: u16) -> Option<&mut Mux> {
        self.links.get_mut(&handle)?.muxes.get_mut(&cid)
    }

    fn records(&self) -> Vec<&'static sdp::Record> {
        Service::ALL
            .into_iter()
            .filter(|s| self.offered.contains(s))
            .flat_map(Service::records)
            .collect()
    }

    fn listening(&self) -> impl Fn(u8) -> bool + use<> {
        let offered = self.offered.clone();
        move |channel| Service::of_channel(channel).is_some_and(|s| offered.contains(&s))
    }
}

fn local_name(name: &str) -> Vec<u8> {
    let mut out = vec![0u8; 248];
    let bytes = name.as_bytes();
    let len = bytes.len().min(247);
    out[..len].copy_from_slice(&bytes[..len]);
    out
}

/// The name and service list a phone sees while searching, before it connects.
fn eir(name: &str, offered: &HashSet<Service>) -> Vec<u8> {
    let base = sdp::full_uuid(0);
    let (mut short, mut long) = (Vec::new(), Vec::new());
    for service in Service::ALL.into_iter().filter(|s| offered.contains(s)) {
        for uuid in service.uuids() {
            if uuid[..2] == [0, 0] && uuid[4..] == base[4..] {
                short.extend_from_slice(&[uuid[3], uuid[2]]);
            } else {
                long.extend(uuid.iter().rev());
            }
        }
    }
    let name = &name.as_bytes()[..name.len().min(48)];
    let mut data = vec![(name.len() + 1) as u8, 0x09];
    data.extend_from_slice(name);
    for (kind, list) in [(0x03u8, short), (0x07, long)] {
        if !list.is_empty() {
            data.push((list.len() + 1) as u8);
            data.push(kind);
            data.extend(list);
        }
    }
    data.resize(240, 0);
    let mut out = vec![0x00];
    out.extend(data);
    out
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self, pkt: &[u8]) -> std::io::Result<()> {
        let mut frame = Vec::with_capacity(2 + pkt.len());
        frame.extend_from_slice(&(pkt.len() as u16).to_be_bytes());
        frame.extend_from_slice(pkt);
        self.tunnel.lock().unwrap_or_else(|e| e.into_inner()).write_all(&frame)
    }

    fn outlet(&self, give: impl FnOnce(&Outlets)) {
        if let Some(outlets) = self.outlets.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            give(outlets);
        }
    }

    fn notice(&self, notice: Notice) {
        self.outlet(|o| {
            let _ = o.notices.send(notice);
        });
    }

    fn end(&self) {
        {
            let mut st = self.state();
            if !st.alive {
                return;
            }
            st.alive = false;
            for (_, mut link) in st.links.drain() {
                link.close();
            }
            for (_, call) in st.calls.drain() {
                let _ = call.socket.send(&[]);
            }
        }
        if let Some(outlets) = self.outlets.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = outlets.notices.send(Notice::Ended);
        }
        let _ = self.tunnel.lock().unwrap_or_else(|e| e.into_inner()).shutdown(Shutdown::Both);
        self.changed.notify_all();
        println!("[bt] the tunnel to the dongle's controller closed");
    }

    fn packet(self: &Arc<Self>, pkt: &[u8]) {
        match pkt.first() {
            Some(&hci::EVENT) => {
                if let Some(event) = hci::event(pkt) {
                    self.event(event);
                }
            }
            Some(&hci::ACL) => {
                if let Some((handle, start, data)) = hci::read_acl(pkt) {
                    self.acl(handle, start, data);
                }
            }
            Some(&hci::SCO) => {
                if let Some((handle, data)) = hci::read_sco(pkt) {
                    self.sco(handle, data);
                }
            }
            _ => {}
        }
        self.changed.notify_all();
    }

    fn event(self: &Arc<Self>, event: Event) {
        let mut guard = self.state();
        let st = &mut *guard;
        match event {
            Event::CommandComplete { opcode, credits, ret } => {
                st.command_credits = credits;
                if opcode != 0 {
                    st.answers.insert(opcode, Ok(ret));
                }
            }
            Event::CommandStatus { opcode, credits, status } => {
                st.command_credits = credits;
                if status != 0 {
                    st.answers.insert(opcode, Err(status));
                    if opcode == hci::CREATE_CONNECTION {
                        for page in st.pages.values_mut().filter(|p| p.is_none()) {
                            *page = Some(Err(status));
                        }
                    }
                }
            }
            Event::ConnectionRequest { addr, link } => {
                if link == hci::LINK_ACL {
                    st.command(hci::ACCEPT_CONNECTION, &[&addr[..], &[REMAIN_PERIPHERAL]].concat());
                } else if st.handle_of(&addr).is_some() {
                    let mut p = addr.to_vec();
                    p.extend_from_slice(&SYNC_BANDWIDTH.to_le_bytes());
                    p.extend_from_slice(&SYNC_BANDWIDTH.to_le_bytes());
                    p.extend_from_slice(&0xffffu16.to_le_bytes());
                    p.extend_from_slice(&VOICE_16BIT.to_le_bytes());
                    p.push(0xff);
                    p.extend_from_slice(&SYNC_PACKETS.to_le_bytes());
                    st.command(hci::ACCEPT_SYNC_CONNECTION, &p);
                } else {
                    st.command(hci::REJECT_SYNC_CONNECTION, &[&addr[..], &[NO_RESOURCES]].concat());
                }
            }
            Event::ConnectionComplete { status, handle, addr, link } if link == hci::LINK_ACL => {
                if let Some(page) = st.pages.get_mut(&addr) {
                    *page = Some(if status == 0 { Ok(handle) } else { Err(status) });
                }
                if status == 0 {
                    st.links.insert(handle, Link::new(addr));
                    println!("[bt] {} connected", text(&addr));
                    drop(guard);
                    self.notice(Notice::Connected(addr));
                } else {
                    println!("[bt] {} would not connect: {}", text(&addr), hci::reason(status));
                }
            }
            Event::DisconnectionComplete { status: 0, handle, reason } => {
                if let Some(mut link) = st.links.remove(&handle) {
                    st.acl_free = (st.acl_free + link.in_flight).min(st.acl_max);
                    st.acl.retain(|(h, _)| *h != handle);
                    link.close();
                    println!("[bt] {} disconnected: {}", text(&link.addr), hci::reason(reason));
                    drop(guard);
                    self.notice(Notice::Disconnected(link.addr));
                } else if let Some(call) = st.calls.remove(&handle) {
                    let _ = call.socket.send(&[]);
                    println!("[bt] call audio closed: {}", hci::reason(reason));
                }
            }
            Event::AuthenticationComplete { status, handle } => {
                if let Some(link) = st.links.get_mut(&handle) {
                    link.auth = Some(status);
                    if status != 0 {
                        println!(
                            "[bt] {} did not authenticate: {}",
                            text(&link.addr),
                            hci::reason(status)
                        );
                    }
                }
            }
            Event::EncryptionChange { status, handle, on } => {
                if let Some(link) = st.links.get_mut(&handle) {
                    link.encryption = Some(status);
                    link.encrypted = status == 0 && on;
                }
            }
            Event::NumberOfCompletedPackets(done) => {
                for (handle, count) in done {
                    if let Some(link) = st.links.get_mut(&handle) {
                        let count = usize::from(count).min(link.in_flight);
                        link.in_flight -= count;
                        st.acl_free = (st.acl_free + count).min(st.acl_max);
                    }
                }
            }
            Event::PinCodeRequest(addr) => {
                println!("[bt] {} asked for a pin, turning it down", text(&addr));
                st.command(hci::PIN_CODE_NEGATIVE_REPLY, &addr);
            }
            Event::LinkKeyRequest(addr) => match st.keys.get(&addr) {
                Some(key) => st.command(hci::LINK_KEY_REPLY, &[&addr[..], &key[..]].concat()),
                None => st.command(hci::LINK_KEY_NEGATIVE_REPLY, &addr),
            },
            Event::LinkKeyNotification { addr, key, kind } => {
                st.keys.put(addr, key, kind);
                println!("[bt] bonded with {}", text(&addr));
                drop(guard);
                self.notice(Notice::Bonded(addr));
            }
            Event::IoCapabilityRequest(addr) => {
                st.command(hci::IO_CAPABILITY_REPLY, &[&addr[..], &IO_CAPABILITY].concat());
            }
            Event::UserConfirmationRequest(addr) => {
                println!("[bt] {} wants to bond, confirming", text(&addr));
                st.command(hci::USER_CONFIRM_REPLY, &addr);
            }
            Event::SimplePairingComplete { status, addr } if status != 0 => {
                println!("[bt] pairing with {} failed: {}", text(&addr), hci::reason(status));
            }
            Event::SyncConnectionComplete(sync) => {
                if sync.status != 0 {
                    println!(
                        "[bt] call audio from {} failed: {}",
                        text(&sync.addr),
                        hci::reason(sync.status)
                    );
                    return;
                }
                let Ok((ours, theirs)) = UnixDatagram::pair() else { return };
                let _ = ours.set_read_timeout(Some(CALL_RECV_WAIT));
                let _ = ours.set_write_timeout(Some(CALL_SEND_WAIT));
                let ours = Arc::new(ours);
                st.calls.insert(sync.handle, CallLink { socket: ours.clone(), seen: false });
                println!(
                    "[bt] {} call audio up over {}, every {} µs, {} B per packet on the air",
                    text(&sync.addr),
                    if sync.link == hci::LINK_ESCO { "eSCO" } else { "SCO" },
                    u32::from(sync.interval) * 625,
                    sync.rx_len
                );
                let shared = self.clone();
                std::thread::spawn(move || call_pump(&shared, sync.handle, &ours));
                drop(guard);
                self.outlet(|o| {
                    let _ = o.calls.send(Call { peer: sync.addr, socket: theirs });
                });
            }
            Event::HardwareError(code) => {
                eprintln!("[bt] the controller reports hardware error {code:#04x}");
            }
            Event::DataBufferOverflow(link) => {
                let kind = if link == hci::LINK_ACL { "data" } else { "call audio" };
                eprintln!("[bt] the controller ran out of room for {kind}");
            }
            _ => {}
        }
    }

    fn sco(&self, handle: u16, data: &[u8]) {
        let socket = {
            let mut st = self.state();
            let Some(call) = st.calls.get_mut(&handle) else { return };
            if !call.seen {
                call.seen = true;
                println!("[bt] call audio arrives in packets of {} B", data.len());
            }
            call.socket.clone()
        };
        let _ = socket.send(data);
    }

    fn acl(self: &Arc<Self>, handle: u16, start: bool, data: &[u8]) {
        let mut guard = self.state();
        let st = &mut *guard;
        let Some(link) = st.links.get_mut(&handle) else { return };
        let Some((cid, payload)) = link.joiner.push(start, data) else { return };
        if cid == l2cap::SIGNALLING {
            let (out, signals) = link
                .l2cap
                .signal(&payload, |psm| psm == l2cap::PSM_SDP || psm == l2cap::PSM_RFCOMM);
            for pdu in out {
                st.push(handle, &pdu);
            }
            for signal in signals {
                signalled(st, handle, signal);
            }
            return;
        }
        let Some(channel) = link.l2cap.channel(cid) else { return };
        match (channel.psm, channel.ours) {
            (l2cap::PSM_SDP, true) => {
                link.sdp_replies.insert(cid, payload);
            }
            (l2cap::PSM_SDP, false) => {
                let reply = sdp::answer(&payload, &st.records());
                st.send_on(handle, cid, &reply);
            }
            (l2cap::PSM_RFCOMM, _) => self.rfcomm(st, handle, cid, &payload),
            _ => {}
        }
    }

    fn rfcomm(self: &Arc<Self>, st: &mut State, handle: u16, cid: u16, payload: &[u8]) {
        let listening = st.listening();
        let Some(link) = st.links.get_mut(&handle) else { return };
        let peer = link.addr;
        let Some(mux) = link.muxes.get_mut(&cid) else { return };
        let (frames, happened) = mux.session.input(payload, listening);
        for frame in frames {
            st.send_on(handle, cid, &frame);
        }
        for event in happened {
            let Some(mux) = st.mux(handle, cid) else { return };
            match event {
                Happened::MuxOpen | Happened::CanSend(_) => {}
                Happened::MuxClosed => {
                    if let Some(mut mux) =
                        st.links.get_mut(&handle).and_then(|l| l.muxes.remove(&cid))
                    {
                        mux.close();
                    }
                    return;
                }
                Happened::Opened(dlci) => {
                    let wanted = mux.wanted.remove(&dlci);
                    let Some(stream) = self.pipe(st, handle, cid, dlci) else { continue };
                    if wanted {
                        if let Some(mux) = st.mux(handle, cid) {
                            mux.ready.insert(dlci, stream);
                        }
                    } else {
                        println!("[bt] {} opened channel {}", text(&peer), dlci >> 1);
                        self.outlet(|o| {
                            let _ = o.incoming.send(Incoming { channel: dlci >> 1, peer, stream });
                        });
                    }
                }
                Happened::Refused(dlci) => {
                    if mux.wanted.remove(&dlci) {
                        mux.refused.insert(dlci);
                    }
                }
                Happened::Closed(dlci) => {
                    if let Some(pipe) = mux.pipes.remove(&dlci) {
                        let _ = pipe.ours.shutdown(Shutdown::Both);
                    }
                }
                Happened::Data(dlci, bytes) => {
                    if let Some(pipe) = mux.pipes.get(&dlci) {
                        let _ = pipe.deliver.send(bytes);
                    }
                }
            }
        }
    }

    /// A socket pair for one RFCOMM channel: what the peer sends comes out of the profile's end,
    /// what the profile writes goes out as frames, as far as credits allow.
    fn pipe(
        self: &Arc<Self>,
        st: &mut State,
        handle: u16,
        cid: u16,
        dlci: u8,
    ) -> Option<UnixStream> {
        let (ours, theirs) = UnixStream::pair().ok()?;
        let (reader, keep) = (ours.try_clone().ok()?, ours.try_clone().ok()?);
        let (deliver, delivered) = mpsc::channel::<Vec<u8>>();
        st.mux(handle, cid)?.pipes.insert(dlci, Pipe { deliver, ours: keep });
        let (a, b) = (self.clone(), self.clone());
        std::thread::spawn(move || hand_in(&a, handle, cid, dlci, ours, &delivered));
        std::thread::spawn(move || hand_out(&b, handle, cid, dlci, reader));
        Some(theirs)
    }

    /// The profile closed its end of a channel.
    fn close_channel(&self, handle: u16, cid: u16, dlci: u8) {
        let mut st = self.state();
        let Some(mux) = st.mux(handle, cid) else { return };
        mux.pipes.remove(&dlci);
        let bye = mux.session.close(dlci);
        // A session we started ends with its last channel.
        let last = mux.session.initiator() && mux.pipes.is_empty() && mux.wanted.is_empty();
        if let Some(bye) = bye {
            st.send_on(handle, cid, &bye);
        }
        if last {
            if let Some(mut mux) = st.links.get_mut(&handle).and_then(|l| l.muxes.remove(&cid)) {
                mux.close();
            }
            let goodbye = st.links.get_mut(&handle).and_then(|l| l.l2cap.disconnect(cid));
            if let Some(goodbye) = goodbye {
                st.push(handle, &goodbye);
            }
        }
        drop(st);
        self.changed.notify_all();
    }
}

fn signalled(st: &mut State, handle: u16, signal: Signal) {
    let Some(link) = st.links.get_mut(&handle) else { return };
    match signal {
        Signal::Opened { local, ours: true, .. } => {
            link.opened.insert(local);
        }
        Signal::Opened { local, psm: l2cap::PSM_RFCOMM, ours: false } => {
            let mtu = link.l2cap.channel(local).map_or(l2cap::OUR_MTU, |c| c.peer_mtu);
            link.muxes.insert(local, Mux::new(Session::new(false, mtu)));
        }
        Signal::Opened { .. } => {}
        Signal::Refused { local } => {
            link.refused.insert(local);
        }
        Signal::Closed { local } => {
            if let Some(mut mux) = link.muxes.remove(&local) {
                mux.close();
            }
            link.sdp_replies.remove(&local);
        }
    }
}

/// What the peer sent, into the profile's end, with credits back as it is taken.
fn hand_in(
    shared: &Shared,
    handle: u16,
    cid: u16,
    dlci: u8,
    mut out: UnixStream,
    delivered: &mpsc::Receiver<Vec<u8>>,
) {
    for bytes in delivered {
        if out.write_all(&bytes).is_err() {
            break;
        }
        let mut st = shared.state();
        let Some(credits) = st.mux(handle, cid).and_then(|m| m.session.consumed(dlci)) else {
            continue;
        };
        st.send_on(handle, cid, &credits);
        drop(st);
        shared.changed.notify_all();
    }
}

/// What the profile writes, out in frames as credits allow.
fn hand_out(shared: &Shared, handle: u16, cid: u16, dlci: u8, mut input: UnixStream) {
    let mut buf = vec![0u8; rfcomm::FRAME];
    loop {
        let size = shared.state().mux(handle, cid).map_or(0, |m| m.session.frame_size(dlci));
        if size == 0 {
            return;
        }
        let size = size.min(buf.len());
        let n = match input.read(&mut buf[..size]) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut st = shared.state();
        loop {
            if !st.alive {
                return;
            }
            let Some(mux) = st.mux(handle, cid) else { return };
            if !mux.session.channel_open(dlci) {
                return;
            }
            if let Some(frame) = mux.session.send(dlci, &buf[..n]) {
                st.send_on(handle, cid, &frame);
                break;
            }
            st = shared.changed.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        drop(st);
        shared.changed.notify_all();
    }
    shared.close_channel(handle, cid, dlci);
}

/// The profile's audio, out as one SCO packet per datagram.
fn call_pump(shared: &Shared, handle: u16, socket: &UnixDatagram) {
    let mut buf = [0u8; 255];
    loop {
        match socket.recv(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                if shared.write(&hci::sco(handle, &buf[..n])).is_err() {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if !shared.state().calls.contains_key(&handle) {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

fn read_tunnel(shared: &Arc<Shared>, mut tunnel: TcpStream) {
    let mut len = [0u8; 2];
    let mut buf = vec![0u8; PACKET_MAX];
    loop {
        if tunnel.read_exact(&mut len).is_err() {
            break;
        }
        let n = usize::from(u16::from_be_bytes(len));
        if n == 0 || n > PACKET_MAX {
            eprintln!("[bt] a frame of {n} bytes from the dongle, closing the tunnel");
            break;
        }
        if tunnel.read_exact(&mut buf[..n]).is_err() {
            break;
        }
        shared.packet(&buf[..n]);
    }
    shared.end();
}

fn write_commands(shared: &Shared) {
    loop {
        let pkt = {
            let mut st = shared.state();
            loop {
                if !st.alive {
                    return;
                }
                if st.command_credits > 0
                    && let Some(pkt) = st.commands.pop_front()
                {
                    st.command_credits -= 1;
                    break pkt;
                }
                let (next, waited) = shared
                    .changed
                    .wait_timeout(st, COMMAND_STALL)
                    .unwrap_or_else(|e| e.into_inner());
                st = next;
                if waited.timed_out() && st.command_credits == 0 && !st.commands.is_empty() {
                    eprintln!("[bt] the controller never answered a command, going on");
                    st.command_credits = 1;
                }
            }
        };
        if shared.write(&pkt).is_err() {
            shared.end();
            return;
        }
    }
}

fn write_acl(shared: &Shared) {
    loop {
        let pkt = {
            let mut st = shared.state();
            loop {
                if !st.alive {
                    return;
                }
                if st.acl_free > 0
                    && let Some((handle, pkt)) = st.acl.pop_front()
                {
                    let Some(link) = st.links.get_mut(&handle) else { continue };
                    link.in_flight += 1;
                    st.acl_free -= 1;
                    break pkt;
                }
                st = shared.changed.wait(st).unwrap_or_else(|e| e.into_inner());
            }
        };
        if shared.write(&pkt).is_err() {
            shared.end();
            return;
        }
    }
}

impl Stack {
    /// Takes the controller behind the tunnel and sets it up, discoverable and connectable.
    pub fn start(tunnel: TcpStream, cfg: Config) -> Result<(Stack, Inbox), String> {
        let reader = tunnel.try_clone().map_err(|e| format!("tunnel: {e}"))?;
        let (incoming, incoming_rx) = mpsc::channel();
        let (calls, calls_rx) = mpsc::channel();
        let (notices, notices_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            tunnel: Mutex::new(tunnel),
            state: Mutex::new(State {
                alive: true,
                name: cfg.name.clone(),
                local: [0; 6],
                acl_mtu: 0,
                acl_free: 0,
                acl_max: 0,
                command_credits: 1,
                commands: VecDeque::new(),
                answers: HashMap::new(),
                acl: VecDeque::new(),
                links: HashMap::new(),
                calls: HashMap::new(),
                pages: HashMap::new(),
                keys: Keys::load(cfg.keys),
                offered: HashSet::new(),
            }),
            changed: Condvar::new(),
            outlets: Mutex::new(Some(Outlets { incoming, calls, notices })),
            paging: Mutex::new(()),
        });
        let (a, b, c) = (shared.clone(), shared.clone(), shared.clone());
        std::thread::spawn(move || read_tunnel(&a, reader));
        std::thread::spawn(move || write_commands(&b));
        std::thread::spawn(move || write_acl(&c));
        let stack = Stack { shared };
        if let Err(e) = stack.setup(&cfg.name) {
            stack.shared.end();
            return Err(e);
        }
        Ok((stack, Inbox { incoming: incoming_rx, calls: calls_rx, notices: notices_rx }))
    }

    fn setup(&self, name: &str) -> Result<(), String> {
        self.command(hci::RESET, &[])?;
        let addr = self.command(hci::READ_BD_ADDR, &[])?;
        let size = self.command(hci::READ_BUFFER_SIZE, &[])?;
        let local: Addr = addr.get(1..7).and_then(|a| a.try_into().ok()).ok_or("no address")?;
        let field =
            |at: usize| size.get(at..at + 2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])));
        let (Some(acl_mtu), Some(acl_max), Some(&sco_mtu)) = (field(1), field(4), size.get(3))
        else {
            return Err("the controller named no buffer sizes".into());
        };
        {
            let mut st = self.shared.state();
            st.local = local;
            st.acl_mtu = acl_mtu.max(27);
            st.acl_max = acl_max.max(1);
            st.acl_free = st.acl_max;
        }
        println!(
            "[bt] the dongle's controller is {}, ACL {acl_mtu} B x {acl_max}, SCO {sco_mtu} B",
            text(&local)
        );
        self.command(hci::SET_EVENT_MASK, &EVENT_MASK)?;
        self.command(hci::WRITE_DEFAULT_LINK_POLICY, &LINK_POLICY.to_le_bytes())?;
        self.command(hci::WRITE_SIMPLE_PAIRING_MODE, &[1])?;
        self.command(hci::WRITE_CLASS_OF_DEVICE, &CLASS)?;
        self.command(hci::WRITE_LOCAL_NAME, &local_name(name))?;
        self.command(hci::WRITE_VOICE_SETTING, &VOICE_16BIT.to_le_bytes())?;
        self.announce()?;
        self.command(hci::WRITE_SCAN_ENABLE, &[SCAN_INQUIRY_AND_PAGE])?;
        Ok(())
    }

    fn wait<T>(
        &self,
        within: Duration,
        mut done: impl FnMut(&mut State) -> Option<T>,
    ) -> Result<T, String> {
        let deadline = Instant::now() + within;
        let mut st = self.shared.state();
        loop {
            if !st.alive {
                return Err("the dongle is gone".into());
            }
            if let Some(value) = done(&mut st) {
                return Ok(value);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("no answer in time".into());
            }
            st = self.shared.changed.wait_timeout(st, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    fn queue(&self, opcode: u16, params: &[u8]) {
        let mut st = self.shared.state();
        st.answers.remove(&opcode);
        st.command(opcode, params);
        drop(st);
        self.shared.changed.notify_all();
    }

    fn command(&self, opcode: u16, params: &[u8]) -> Result<Vec<u8>, String> {
        self.queue(opcode, params);
        let answer = self
            .wait(COMMAND_WAIT, |st| st.answers.remove(&opcode))
            .map_err(|e| format!("{opcode:#06x}: {e}"))?;
        match answer {
            Ok(ret) if ret.first().is_none_or(|status| *status == 0) => Ok(ret),
            Ok(ret) => Err(format!("{opcode:#06x} refused: {}", hci::reason(ret[0]))),
            Err(status) => Err(format!("{opcode:#06x} refused: {}", hci::reason(status))),
        }
    }

    fn announce(&self) -> Result<(), String> {
        let data = {
            let st = self.shared.state();
            eir(&st.name, &st.offered)
        };
        self.command(hci::WRITE_EXTENDED_INQUIRY_RESPONSE, &data).map(|_| ())
    }

    pub fn local(&self) -> Addr {
        self.shared.state().local
    }

    pub fn alive(&self) -> bool {
        self.shared.state().alive
    }

    /// Closes the tunnel, every channel and call with it.
    pub fn end(&self) {
        self.shared.end();
    }

    /// Lists a service's records and channels, or takes them back.
    pub fn offer(&self, service: Service, on: bool) -> Result<(), String> {
        let changed = {
            let mut st = self.shared.state();
            if on { st.offered.insert(service) } else { st.offered.remove(&service) }
        };
        if !changed {
            return Ok(());
        }
        for record in service.records() {
            let verb = if on { "offering" } else { "withdrew" };
            println!("[bt] {verb} {} on channel {}", record.name, record.channel);
        }
        self.announce()
    }

    pub fn bonded(&self) -> Vec<Addr> {
        self.shared.state().keys.bonded()
    }

    pub fn connected(&self, peer: &Addr) -> bool {
        self.shared.state().handle_of(peer).is_some()
    }

    /// Whether a channel with the phone is open, whichever side opened it.
    pub fn in_use(&self, peer: &Addr) -> bool {
        let st = self.shared.state();
        st.links
            .values()
            .filter(|l| l.addr == *peer)
            .any(|l| l.muxes.values().any(|m| !m.pipes.is_empty()))
    }

    pub fn disconnect(&self, peer: &Addr) -> bool {
        let handle = self.shared.state().handle_of(peer);
        if let Some(handle) = handle {
            self.queue(hci::DISCONNECT, &[&handle.to_le_bytes()[..], &[USER_ENDED]].concat());
        }
        handle.is_some()
    }

    pub fn forget(&self, peer: &Addr) {
        self.shared.state().keys.forget(peer);
    }

    /// Pages the phone if need be, then opens the RFCOMM channel its service record names.
    /// `service` is the SDP element of the UUID to look for.
    pub fn connect(&self, peer: Addr, service: &[u8]) -> Result<UnixStream, String> {
        let _one = self.shared.paging.lock().unwrap_or_else(|e| e.into_inner());
        let handle = self.link_to(peer)?;
        self.secure(handle)?;
        let channel = self.find_channel(handle, service)?;
        let cid = self.session_on(handle)?;
        self.open_channel(handle, cid, channel)
    }

    fn link_to(&self, peer: Addr) -> Result<u16, String> {
        {
            let mut st = self.shared.state();
            if let Some(handle) = st.handle_of(&peer) {
                return Ok(handle);
            }
            st.pages.insert(peer, None);
        }
        let mut p = peer.to_vec();
        p.extend_from_slice(&ACL_PACKETS.to_le_bytes());
        // R2 page scan, a reserved byte, no clock offset, role switch allowed.
        p.extend_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x01]);
        self.queue(hci::CREATE_CONNECTION, &p);
        let paged = self.wait(PAGE_WAIT, |st| {
            if let Some(handle) = st.handle_of(&peer) {
                return Some(Ok(handle));
            }
            st.pages.get(&peer).copied().flatten()
        });
        self.shared.state().pages.remove(&peer);
        match paged? {
            Ok(handle) => Ok(handle),
            Err(status) => Err(hci::reason(status).into()),
        }
    }

    /// Authentication and encryption, as a phone wants them before it opens a channel.
    fn secure(&self, handle: u16) -> Result<(), String> {
        {
            let mut st = self.shared.state();
            let link = st.links.get_mut(&handle).ok_or("the link went")?;
            if link.encrypted {
                return Ok(());
            }
            link.auth = None;
            link.encryption = None;
        }
        self.queue(hci::AUTHENTICATION_REQUESTED, &handle.to_le_bytes());
        let auth = self.wait(SECURE_WAIT, |st| match st.links.get(&handle) {
            None => Some(Err("the link went".to_string())),
            Some(link) => link.auth.map(Ok),
        })??;
        if auth != 0 {
            return Err(format!("authentication failed: {}", hci::reason(auth)));
        }
        self.queue(hci::SET_CONNECTION_ENCRYPTION, &[&handle.to_le_bytes()[..], &[1]].concat());
        let encryption = self.wait(STEP_WAIT, |st| match st.links.get(&handle) {
            None => Some(Err("the link went".to_string())),
            Some(link) if link.encrypted => Some(Ok(0)),
            Some(link) => link.encryption.map(Ok),
        })??;
        if encryption != 0 {
            return Err(format!("encryption failed: {}", hci::reason(encryption)));
        }
        Ok(())
    }

    fn open_l2cap(&self, handle: u16, psm: u16) -> Result<u16, String> {
        let local = {
            let mut st = self.shared.state();
            let link = st.links.get_mut(&handle).ok_or("the link went")?;
            let (local, ask) = link.l2cap.connect(psm);
            st.push(handle, &ask);
            local
        };
        self.shared.changed.notify_all();
        self.wait(STEP_WAIT, |st| {
            let Some(link) = st.links.get_mut(&handle) else {
                return Some(Err("the link went".to_string()));
            };
            if link.opened.remove(&local) {
                return Some(Ok(local));
            }
            link.refused.remove(&local).then(|| Err(format!("the phone turned channel {psm} down")))
        })?
    }

    fn close_l2cap(&self, handle: u16, local: u16) {
        let mut st = self.shared.state();
        let bye = st.links.get_mut(&handle).and_then(|l| l.l2cap.disconnect(local));
        if let Some(bye) = bye {
            st.push(handle, &bye);
        }
        drop(st);
        self.shared.changed.notify_all();
    }

    fn find_channel(&self, handle: u16, service: &[u8]) -> Result<u8, String> {
        let local = self.open_l2cap(handle, l2cap::PSM_SDP)?;
        {
            let mut st = self.shared.state();
            st.send_on(handle, local, &sdp::ask_channel(1, service));
        }
        self.shared.changed.notify_all();
        let reply = self.wait(STEP_WAIT, |st| match st.links.get_mut(&handle) {
            None => Some(None),
            Some(link) => link.sdp_replies.remove(&local).map(Some),
        });
        self.close_l2cap(handle, local);
        let reply = reply?.ok_or("the link went")?;
        sdp::channel_in(&reply).ok_or_else(|| "the phone offers no such service".into())
    }

    /// The RFCOMM session to the phone, the one already there or a new one of ours.
    fn session_on(&self, handle: u16) -> Result<u16, String> {
        {
            let st = self.shared.state();
            let link = st.links.get(&handle).ok_or("the link went")?;
            if let Some((cid, _)) = link.muxes.iter().find(|(_, m)| m.session.is_open()) {
                return Ok(*cid);
            }
        }
        let local = self.open_l2cap(handle, l2cap::PSM_RFCOMM)?;
        {
            let mut st = self.shared.state();
            let link = st.links.get_mut(&handle).ok_or("the link went")?;
            let mtu = link.l2cap.channel(local).map_or(l2cap::OUR_MTU, |c| c.peer_mtu);
            let mut session = Session::new(true, mtu);
            let start = session.start();
            link.muxes.insert(local, Mux::new(session));
            st.send_on(handle, local, &start);
        }
        self.shared.changed.notify_all();
        self.wait(STEP_WAIT, |st| match st.mux(handle, local) {
            None => Some(Err("the phone refused RFCOMM".to_string())),
            Some(mux) => mux.session.is_open().then_some(Ok(local)),
        })?
    }

    fn open_channel(&self, handle: u16, cid: u16, channel: u8) -> Result<UnixStream, String> {
        let dlci = {
            let mut st = self.shared.state();
            let mux = st.mux(handle, cid).ok_or("the session went")?;
            let dlci = mux.session.dlci(channel, true);
            mux.wanted.insert(dlci);
            let ask = mux.session.open(channel);
            st.send_on(handle, cid, &ask);
            dlci
        };
        self.shared.changed.notify_all();
        self.wait(STEP_WAIT, |st| match st.mux(handle, cid) {
            None => Some(Err("the session went".to_string())),
            Some(mux) => {
                if let Some(stream) = mux.ready.remove(&dlci) {
                    return Some(Ok(stream));
                }
                mux.refused
                    .remove(&dlci)
                    .then(|| Err(format!("the phone turned channel {channel} down")))
            }
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_answer_names_us_and_what_we_offer() {
        let offered: HashSet<Service> = [Service::HandsFree, Service::AndroidAuto].into();
        let data = eir("LIVI", &offered);
        assert_eq!(data.len(), 241);
        assert_eq!(&data[1..7], &[5, 0x09, b'L', b'I', b'V', b'I']);
        assert_eq!(&data[7..11], &[3, 0x03, 0x1e, 0x11]);
        assert_eq!(&data[11..13], &[17, 0x07]);
        let mut aa = sdp::ANDROID_AUTO_UUID;
        aa.reverse();
        assert_eq!(&data[13..29], &aa);
    }

    #[test]
    fn a_long_name_is_cut_to_the_field() {
        assert_eq!(local_name("LIVI").len(), 248);
        assert_eq!(&local_name("LIVI")[..5], b"LIVI\0");
        assert_eq!(local_name(&"x".repeat(300))[247], 0);
    }
}
