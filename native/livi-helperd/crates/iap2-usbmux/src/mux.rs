// usbmux over the CarPlay bulk interface: framed multiplexer with a minimal TCP on top.
// Lockdown lives on TCP port 62078.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::backend;
use crate::pipe::{MuxReader, MuxWriter};

const P_VERSION: u32 = 0;
const P_SETUP: u32 = 2;
const P_TCP: u32 = 6;
const MUX_MAGIC: u32 = 0xFEED_FACE;
const TX_WIN: u32 = 131072;
const MAX_PAYLOAD: usize = 16384;

pub const LOCKDOWN_PORT: u16 = 62078;

const TH_FIN: u8 = 0x01;
const TH_SYN: u8 = 0x02;
const TH_RST: u8 = 0x04;
const TH_ACK: u8 = 0x10;

struct ConnState {
    tx_seq: u32,
    tx_ack: u32,
    connected: bool,
    closed: bool,
    rq: std::collections::VecDeque<Vec<u8>>,
}

pub struct MuxTcpConn {
    host: Arc<MuxHost>,
    sport: u16,
    dport: u16,
    state: Mutex<ConnState>,
    cv: Condvar,
}

impl MuxTcpConn {
    fn tcp(&self, flags: u8, payload: &[u8]) {
        let (seq, ack) = {
            let s = self.state.lock().unwrap();
            (s.tx_seq, s.tx_ack)
        };
        let mut th = Vec::with_capacity(20 + payload.len());
        th.extend_from_slice(&self.sport.to_be_bytes());
        th.extend_from_slice(&self.dport.to_be_bytes());
        th.extend_from_slice(&seq.to_be_bytes());
        th.extend_from_slice(&ack.to_be_bytes());
        th.push(0x50);
        th.push(flags);
        th.extend_from_slice(&((TX_WIN >> 8) as u16).to_be_bytes());
        th.extend_from_slice(&[0, 0, 0, 0]);
        th.extend_from_slice(payload);
        self.host.mux_send(P_TCP, &th);
    }

    fn on_packet(&self, flags: u8, seq: u32, _ack: u32, payload: &[u8]) {
        let mut s = self.state.lock().unwrap();
        if flags & TH_SYN != 0 && flags & TH_ACK != 0 {
            s.tx_seq = s.tx_seq.wrapping_add(1);
            s.tx_ack = seq.wrapping_add(1);
            drop(s);
            self.tcp(TH_ACK, &[]);
            let mut s = self.state.lock().unwrap();
            s.connected = true;
            self.cv.notify_all();
            return;
        }
        if flags & TH_RST != 0 {
            s.closed = true;
            s.connected = true;
            s.rq.push_back(Vec::new());
            self.cv.notify_all();
            return;
        }
        if !payload.is_empty() {
            s.tx_ack = s.tx_ack.wrapping_add(payload.len() as u32);
            s.rq.push_back(payload.to_vec());
            drop(s);
            self.tcp(TH_ACK, &[]);
            self.cv.notify_all();
            s = self.state.lock().unwrap();
        }
        if flags & TH_FIN != 0 {
            s.tx_ack = s.tx_ack.wrapping_add(1);
            drop(s);
            self.tcp(TH_ACK, &[]);
            let mut s = self.state.lock().unwrap();
            s.closed = true;
            s.rq.push_back(Vec::new());
            self.cv.notify_all();
        }
    }

    pub fn send(&self, data: &[u8]) {
        for chunk in data.chunks(MAX_PAYLOAD) {
            self.tcp(TH_ACK, chunk);
            let mut s = self.state.lock().unwrap();
            s.tx_seq = s.tx_seq.wrapping_add(chunk.len() as u32);
        }
    }

    pub fn recv(&self, timeout: Duration) -> Option<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        let mut s = self.state.lock().unwrap();
        loop {
            if let Some(data) = s.rq.pop_front() {
                return Some(data);
            }
            if s.closed {
                return Some(Vec::new());
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (guard, _) = self.cv.wait_timeout(s, deadline - now).unwrap();
            s = guard;
        }
    }

    pub fn close(&self) {
        let closed = {
            let s = self.state.lock().unwrap();
            s.closed
        };
        if !closed {
            self.tcp(TH_FIN | TH_ACK, &[]);
        }
        let mut s = self.state.lock().unwrap();
        s.closed = true;
        s.rq.push_back(Vec::new());
        self.cv.notify_all();
    }
}

pub struct MuxHost {
    ep_out: Mutex<Box<dyn MuxWriter>>,
    ep_in: Mutex<Option<Box<dyn MuxReader>>>,
    tx: Mutex<u16>,
    conns: Mutex<HashMap<u16, Arc<MuxTcpConn>>>,
    next_sport: AtomicU16,
    run: Arc<AtomicBool>,
}

impl MuxHost {
    pub fn open(serial: &str) -> Result<Arc<Self>, String> {
        let (mut ep_out, mut ep_in) = backend::open_pipes(serial)?;

        // Version handshake (protocol 2), then SETUP.
        let mut version = Vec::new();
        version.extend_from_slice(&P_VERSION.to_be_bytes());
        version.extend_from_slice(&20u32.to_be_bytes());
        version.extend_from_slice(&2u32.to_be_bytes());
        version.extend_from_slice(&[0u8; 8]);
        ep_out.write(&version)?;
        let _ = ep_in.read(Duration::from_millis(2000));

        let host = Arc::new(Self {
            ep_out: Mutex::new(ep_out),
            ep_in: Mutex::new(Some(ep_in)),
            tx: Mutex::new(0),
            conns: Mutex::new(HashMap::new()),
            next_sport: AtomicU16::new(1),
            run: Arc::new(AtomicBool::new(true)),
        });
        host.mux_send(P_SETUP, &[0x07]);
        host.clone().spawn_reader();
        Ok(host)
    }

    fn mux_send(&self, proto: u32, payload: &[u8]) {
        let mut tx = self.tx.lock().unwrap();
        let mut pkt = Vec::with_capacity(16 + payload.len());
        pkt.extend_from_slice(&proto.to_be_bytes());
        pkt.extend_from_slice(&((16 + payload.len()) as u32).to_be_bytes());
        pkt.extend_from_slice(&MUX_MAGIC.to_be_bytes());
        pkt.extend_from_slice(&tx.to_be_bytes());
        pkt.extend_from_slice(&0u16.to_be_bytes());
        pkt.extend_from_slice(payload);
        *tx = tx.wrapping_add(1);
        let _ = self.ep_out.lock().unwrap().write(&pkt);
    }

    fn spawn_reader(self: Arc<Self>) {
        let Some(mut ep_in) = self.ep_in.lock().unwrap().take() else {
            return;
        };
        std::thread::spawn(move || {
            let mut rxbuf: Vec<u8> = Vec::new();
            while self.run.load(Ordering::SeqCst) {
                let data = match ep_in.read(Duration::from_millis(1000)) {
                    Ok(d) => d,
                    Err(_) => break,
                };
                if data.is_empty() {
                    continue;
                }
                rxbuf.extend_from_slice(&data);
                while rxbuf.len() >= 8 {
                    let proto = u32::from_be_bytes(rxbuf[0..4].try_into().unwrap());
                    let length = u32::from_be_bytes(rxbuf[4..8].try_into().unwrap()) as usize;
                    if length < 8 || rxbuf.len() < length {
                        break;
                    }
                    let pkt: Vec<u8> = rxbuf.drain(..length).collect();
                    if proto == P_TCP && length >= 36 {
                        let dport = u16::from_be_bytes([pkt[18], pkt[19]]);
                        let seq = u32::from_be_bytes(pkt[20..24].try_into().unwrap());
                        let ack = u32::from_be_bytes(pkt[24..28].try_into().unwrap());
                        let flags = pkt[29];
                        let conn = self.conns.lock().unwrap().get(&dport).cloned();
                        if let Some(conn) = conn {
                            conn.on_packet(flags, seq, ack, &pkt[36..length]);
                        }
                    }
                }
            }
        });
    }

    pub fn connect(self: &Arc<Self>, dport: u16) -> Result<Arc<MuxTcpConn>, String> {
        let sport = self.next_sport.fetch_add(1, Ordering::SeqCst);
        let conn = Arc::new(MuxTcpConn {
            host: self.clone(),
            sport,
            dport,
            state: Mutex::new(ConnState {
                tx_seq: 0,
                tx_ack: 0,
                connected: false,
                closed: false,
                rq: std::collections::VecDeque::new(),
            }),
            cv: Condvar::new(),
        });
        self.conns.lock().unwrap().insert(sport, conn.clone());
        conn.tcp(TH_SYN, &[]);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut s = conn.state.lock().unwrap();
        while !s.connected {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (guard, _) = conn.cv.wait_timeout(s, deadline - now).unwrap();
            s = guard;
        }
        let ok = s.connected && !s.closed;
        drop(s);
        if !ok {
            self.conns.lock().unwrap().remove(&sport);
            return Err(format!("mux connect to port {dport} failed"));
        }
        Ok(conn)
    }
}

impl Drop for MuxHost {
    fn drop(&mut self) {
        self.run.store(false, Ordering::SeqCst);
    }
}
