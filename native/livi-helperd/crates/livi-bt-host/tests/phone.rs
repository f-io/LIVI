//! A made-up controller behind the tunnel, and a phone on the other side of it.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use livi_bt_host::rfcomm::{Happened, Session};
use livi_bt_host::sdp::{self, Service};
use livi_bt_host::{Config, Notice, Stack, hci, l2cap};

const LOCAL: hci::Addr = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
const PHONE: hci::Addr = [1, 2, 3, 4, 5, 6];
const HANDLE: u16 = 0x002a;
const ACL_MTU: u16 = 1021;
const WAIT: Duration = Duration::from_secs(3);

fn framed(pkt: &[u8]) -> Vec<u8> {
    let mut out = (pkt.len() as u16).to_be_bytes().to_vec();
    out.extend_from_slice(pkt);
    out
}

fn event(code: u8, params: &[u8]) -> Vec<u8> {
    let mut out = vec![hci::EVENT, code, params.len() as u8];
    out.extend_from_slice(params);
    out
}

fn complete(opcode: u16, ret: &[u8]) -> Vec<u8> {
    let mut params = vec![1];
    params.extend_from_slice(&opcode.to_le_bytes());
    params.extend_from_slice(ret);
    event(0x0e, &params)
}

/// Answers every command, gives every ACL packet its credit back and passes it on.
fn controller(mut input: TcpStream, out: Arc<Mutex<TcpStream>>, acl: mpsc::Sender<Vec<u8>>) {
    let send = |pkt: Vec<u8>| out.lock().unwrap().write_all(&framed(&pkt)).unwrap();
    loop {
        let mut len = [0u8; 2];
        if input.read_exact(&mut len).is_err() {
            return;
        }
        let mut pkt = vec![0u8; usize::from(u16::from_be_bytes(len))];
        if input.read_exact(&mut pkt).is_err() {
            return;
        }
        match pkt[0] {
            hci::COMMAND => {
                let opcode = u16::from_le_bytes([pkt[1], pkt[2]]);
                match opcode {
                    hci::READ_BD_ADDR => send(complete(opcode, &[[0].as_slice(), &LOCAL].concat())),
                    hci::READ_BUFFER_SIZE => {
                        let mut ret = vec![0];
                        ret.extend_from_slice(&ACL_MTU.to_le_bytes());
                        ret.extend_from_slice(&[48, 8, 0, 8, 0]);
                        send(complete(opcode, &ret));
                    }
                    hci::ACCEPT_CONNECTION | hci::CREATE_CONNECTION => {
                        let [lo, hi] = opcode.to_le_bytes();
                        send(event(0x0f, &[0, 1, lo, hi]));
                        let mut params = vec![0];
                        params.extend_from_slice(&HANDLE.to_le_bytes());
                        params.extend_from_slice(&PHONE);
                        params.extend_from_slice(&[hci::LINK_ACL, 0]);
                        send(event(0x03, &params));
                    }
                    hci::AUTHENTICATION_REQUESTED => {
                        let [lo, hi] = opcode.to_le_bytes();
                        send(event(0x0f, &[0, 1, lo, hi]));
                        send(event(0x06, &[&[0u8][..], &HANDLE.to_le_bytes()].concat()));
                    }
                    hci::SET_CONNECTION_ENCRYPTION => {
                        let [lo, hi] = opcode.to_le_bytes();
                        send(event(0x0f, &[0, 1, lo, hi]));
                        send(event(0x08, &[&[0u8][..], &HANDLE.to_le_bytes(), &[1]].concat()));
                    }
                    _ => send(complete(opcode, &[0])),
                }
            }
            hci::ACL => {
                let mut done = vec![1];
                done.extend_from_slice(&HANDLE.to_le_bytes());
                done.extend_from_slice(&1u16.to_le_bytes());
                send(event(0x13, &done));
                if acl.send(pkt).is_err() {
                    return;
                }
            }
            _ => {}
        }
    }
}

struct Phone {
    out: Arc<Mutex<TcpStream>>,
    acl: mpsc::Receiver<Vec<u8>>,
    joiner: l2cap::Joiner,
}

impl Phone {
    fn event(&self, pkt: Vec<u8>) {
        self.out.lock().unwrap().write_all(&framed(&pkt)).unwrap();
    }

    fn l2cap(&self, cid: u16, payload: &[u8]) {
        for pkt in l2cap::split(HANDLE, &l2cap::pdu(cid, payload), usize::from(ACL_MTU)) {
            self.event(pkt);
        }
    }

    fn signal(&self, code: u8, ident: u8, data: &[u8]) {
        let mut cmd = vec![code, ident];
        cmd.extend_from_slice(&(data.len() as u16).to_le_bytes());
        cmd.extend_from_slice(data);
        self.l2cap(l2cap::SIGNALLING, &cmd);
    }

    fn next(&mut self) -> (u16, Vec<u8>) {
        loop {
            let pkt = self.acl.recv_timeout(WAIT).expect("the host went quiet");
            let (handle, start, data) = hci::read_acl(&pkt).unwrap();
            assert_eq!(handle, HANDLE);
            if let Some(pdu) = self.joiner.push(start, data) {
                return pdu;
            }
        }
    }

    /// Feeds the host's frames to the phone's session and answers them, until `until` happened.
    fn talk(&mut self, session: &mut Session, cid: u16, until: &Happened) {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            let (on, frame) = self.next();
            assert_eq!(on, 0x0041);
            let (out, happened) = session.input(&frame, |_| false);
            for f in out {
                self.l2cap(cid, &f);
            }
            if happened.contains(until) {
                return;
            }
        }
        panic!("never saw {until:?}");
    }
}

fn start() -> (Stack, livi_bt_host::Inbox, Phone, std::path::PathBuf) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host_end = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (controller_end, _) = listener.accept().unwrap();
    let out = Arc::new(Mutex::new(controller_end.try_clone().unwrap()));
    let (acl_tx, acl) = mpsc::channel();
    let feed = out.clone();
    std::thread::spawn(move || controller(controller_end, feed, acl_tx));
    let keys = std::env::temp_dir().join(format!(
        "livi-bt-phone-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let (stack, inbox) =
        Stack::start(host_end, Config { name: "LIVI".into(), keys: keys.clone() }).unwrap();
    (stack, inbox, Phone { out, acl, joiner: l2cap::Joiner::default() }, keys)
}

fn le(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn a_phone_opens_hands_free_and_talks_over_it() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host_end = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (controller_end, _) = listener.accept().unwrap();
    let out = Arc::new(Mutex::new(controller_end.try_clone().unwrap()));
    let (acl_tx, acl) = mpsc::channel();
    let feed = out.clone();
    std::thread::spawn(move || controller(controller_end, feed, acl_tx));

    let keys = std::env::temp_dir().join(format!("livi-bt-phone-{}", std::process::id()));
    let (stack, inbox) =
        Stack::start(host_end, Config { name: "LIVI".into(), keys: keys.clone() }).unwrap();
    assert_eq!(stack.local(), LOCAL);
    stack.offer(Service::HandsFree, true).unwrap();
    let mut phone = Phone { out, acl, joiner: l2cap::Joiner::default() };

    phone.event(event(0x04, &[&PHONE[..], &[0x0c, 0x02, 0x5a, hci::LINK_ACL]].concat()));
    assert_eq!(inbox.notices.recv_timeout(WAIT), Ok(Notice::Connected(PHONE)));
    assert!(stack.connected(&PHONE));

    // L2CAP for RFCOMM, the phone's end is 0x0041.
    phone.signal(0x02, 1, &le(&[l2cap::PSM_RFCOMM, 0x0041]));
    let (_, answer) = phone.next();
    assert_eq!(answer[0], 0x03);
    let host_cid = u16::from_le_bytes([answer[4], answer[5]]);
    let (_, asked) = phone.next();
    assert_eq!(asked[0], 0x04);
    phone.signal(0x05, asked[1], &le(&[host_cid, 0, 0]));
    let mut mtu = le(&[host_cid, 0]);
    mtu.extend_from_slice(&[0x01, 0x02]);
    mtu.extend_from_slice(&l2cap::OUR_MTU.to_le_bytes());
    phone.signal(0x04, 2, &mtu);
    let (_, configured) = phone.next();
    assert_eq!(configured[0], 0x05);

    let mut session = Session::new(true, l2cap::OUR_MTU);
    phone.l2cap(host_cid, &session.start());
    phone.talk(&mut session, host_cid, &Happened::MuxOpen);
    phone.l2cap(host_cid, &session.open(7));
    phone.talk(&mut session, host_cid, &Happened::Opened(14));

    let incoming = inbox.incoming.recv_timeout(WAIT).expect("no channel handed over");
    assert_eq!((incoming.channel, incoming.peer), (7, PHONE));
    let mut profile = incoming.stream;
    profile.set_read_timeout(Some(WAIT)).unwrap();

    // Far more frames than the first credits cover, so the host has to hand credits back.
    for _ in 0..20 {
        let frame = loop {
            if let Some(frame) = session.send(14, b"AT+BRSF=1\r") {
                break frame;
            }
            phone.talk(&mut session, host_cid, &Happened::CanSend(14));
        };
        phone.l2cap(host_cid, &frame);
        let mut got = [0u8; 10];
        profile.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"AT+BRSF=1\r");
    }

    profile.write_all(b"\r\nOK\r\n").unwrap();
    phone.talk(&mut session, host_cid, &Happened::Data(14, b"\r\nOK\r\n".to_vec()));

    drop(profile);
    phone.talk(&mut session, host_cid, &Happened::Closed(14));
    let _ = std::fs::remove_file(keys);
}

/// The phone side of our own page: its L2CAP answers, its SDP record and its RFCOMM channel 13.
#[test]
fn we_page_a_phone_and_open_its_gateway() {
    let (stack, inbox, mut phone, keys) = start();
    let pager = stack.clone();
    let opened =
        std::thread::spawn(move || pager.connect(PHONE, &sdp::uuid_of(sdp::HANDS_FREE_GATEWAY)));
    assert_eq!(inbox.notices.recv_timeout(WAIT), Ok(Notice::Connected(PHONE)));

    let gateway = sdp::Record {
        handle: 0x0001_0009,
        uuid: sdp::full_uuid(sdp::HANDS_FREE_GATEWAY),
        channel: 13,
        name: "Gateway",
        profile: sdp::Profile::SerialPort,
    };
    let mut link = l2cap::Link::default();
    let mut session: Option<(u16, Session)> = None;
    let deadline = Instant::now() + WAIT;
    let stream = loop {
        assert!(Instant::now() < deadline, "the page never got through");
        if opened.is_finished() {
            break opened.join().unwrap().expect("our page failed");
        }
        let Ok(pkt) = phone.acl.recv_timeout(Duration::from_millis(50)) else { continue };
        let (_, start, data) = hci::read_acl(&pkt).unwrap();
        let Some((cid, payload)) = phone.joiner.push(start, data) else { continue };
        if cid == l2cap::SIGNALLING {
            let (out, signals) = link.signal(&payload, |_| true);
            for pdu in out {
                for pkt in l2cap::split(HANDLE, &pdu, usize::from(ACL_MTU)) {
                    phone.event(pkt);
                }
            }
            for signal in signals {
                if let l2cap::Signal::Opened { local, psm: l2cap::PSM_RFCOMM, .. } = signal {
                    session = Some((local, Session::new(false, l2cap::OUR_MTU)));
                }
            }
            continue;
        }
        let channel = link.channel(cid).expect("a channel the phone knows");
        let remote = channel.remote;
        if channel.psm == l2cap::PSM_SDP {
            phone.l2cap(remote, &sdp::answer(&payload, &[&gateway]));
        } else if let Some((_, s)) = session.as_mut() {
            let (out, _) = s.input(&payload, |c| c == 13);
            for f in out {
                phone.l2cap(remote, &f);
            }
        }
    };
    let (rfcomm_cid, mut s) = session.expect("an RFCOMM session");
    assert!(s.channel_open(26), "its channel 13 on the session we started");
    let mut ours = stream;
    ours.write_all(b"AT+BRSF=156\r").unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        assert!(Instant::now() < deadline, "nothing came through the channel");
        let (cid, payload) = phone.next();
        if cid != rfcomm_cid {
            continue;
        }
        let (_, happened) = s.input(&payload, |_| false);
        if happened.contains(&Happened::Data(26, b"AT+BRSF=156\r".to_vec())) {
            break;
        }
    }
    let _ = std::fs::remove_file(keys);
}
