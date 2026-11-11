// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use livi_session_io::usb::UsbStream;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf, ReadHalf};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::PollSender;

use crate::{Error, backend};

const P_VERSION: u32 = 0;
const P_SETUP: u32 = 2;
const P_TCP: u32 = 6;
const MUX_MAGIC: u32 = 0xFEED_FACE;
const TX_WIN: u32 = 131072;
const MAX_PAYLOAD: usize = 16384;
const MAX_PACKET: usize = 1 << 20;
const READ_CHUNK: usize = 65536;
const VERSION_WAIT: Duration = Duration::from_secs(2);
const CONNECT_WAIT: Duration = Duration::from_secs(5);
const QUEUED: usize = 64;

pub const LOCKDOWN_PORT: u16 = 62078;

const TH_FIN: u8 = 0x01;
const TH_SYN: u8 = 0x02;
const TH_RST: u8 = 0x04;
const TH_ACK: u8 = 0x10;

type Out = mpsc::Sender<(u32, Vec<u8>)>;

struct ConnState {
    tx_seq: u32,
    tx_ack: u32,
    closed: bool,
    up: Option<oneshot::Sender<()>>,
    rx: Option<mpsc::Sender<Vec<u8>>>,
}

#[derive(Default)]
struct Reaction {
    data: Option<(mpsc::Sender<Vec<u8>>, Vec<u8>)>,
    replies: Vec<Vec<u8>>,
}

struct Conn {
    sport: u16,
    dport: u16,
    state: Mutex<ConnState>,
}

impl Conn {
    fn segment(&self, s: &ConnState, flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut th = Vec::with_capacity(20 + payload.len());
        th.extend_from_slice(&self.sport.to_be_bytes());
        th.extend_from_slice(&self.dport.to_be_bytes());
        th.extend_from_slice(&s.tx_seq.to_be_bytes());
        th.extend_from_slice(&s.tx_ack.to_be_bytes());
        th.push(0x50);
        th.push(flags);
        th.extend_from_slice(&((TX_WIN >> 8) as u16).to_be_bytes());
        th.extend_from_slice(&[0, 0, 0, 0]);
        th.extend_from_slice(payload);
        th
    }

    fn on_packet(&self, flags: u8, seq: u32, payload: &[u8]) -> Reaction {
        let mut s = self.state.lock().unwrap();
        let mut reaction = Reaction::default();
        if flags & TH_SYN != 0 && flags & TH_ACK != 0 {
            s.tx_seq = s.tx_seq.wrapping_add(1);
            s.tx_ack = seq.wrapping_add(1);
            reaction.replies.push(self.segment(&s, TH_ACK, &[]));
            if let Some(up) = s.up.take() {
                let _ = up.send(());
            }
            return reaction;
        }
        if flags & TH_RST != 0 {
            end(&mut s);
            return reaction;
        }
        if !payload.is_empty() {
            s.tx_ack = s.tx_ack.wrapping_add(payload.len() as u32);
            reaction.data = s.rx.clone().map(|rx| (rx, payload.to_vec()));
            reaction.replies.push(self.segment(&s, TH_ACK, &[]));
        }
        if flags & TH_FIN != 0 {
            s.tx_ack = s.tx_ack.wrapping_add(1);
            reaction.replies.push(self.segment(&s, TH_ACK, &[]));
            end(&mut s);
        }
        reaction
    }

    fn data(&self, chunk: &[u8]) -> Vec<u8> {
        let mut s = self.state.lock().unwrap();
        let segment = self.segment(&s, TH_ACK, chunk);
        s.tx_seq = s.tx_seq.wrapping_add(chunk.len() as u32);
        segment
    }

    fn fin(&self) -> Option<Vec<u8>> {
        let mut s = self.state.lock().unwrap();
        let fin = (!s.closed).then(|| self.segment(&s, TH_FIN | TH_ACK, &[]));
        end(&mut s);
        fin
    }

    fn close(&self, out: &Out) {
        if let Some(fin) = self.fin() {
            send_soon(out, fin);
        }
    }
}

fn send_soon(out: &Out, segment: Vec<u8>) {
    if let Err(mpsc::error::TrySendError::Full(packet)) = out.try_send((P_TCP, segment))
        && let Ok(rt) = tokio::runtime::Handle::try_current()
    {
        let out = out.clone();
        rt.spawn(async move {
            let _ = out.send(packet).await;
        });
    }
}

fn end(s: &mut ConnState) {
    s.closed = true;
    s.up = None;
    s.rx = None;
}

type Conns = Arc<Mutex<HashMap<u16, Arc<Conn>>>>;

pub struct MuxHost {
    out: Out,
    conns: Conns,
    next_sport: AtomicU16,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

fn version_packet() -> Vec<u8> {
    let mut version = Vec::with_capacity(20);
    version.extend_from_slice(&P_VERSION.to_be_bytes());
    version.extend_from_slice(&20u32.to_be_bytes());
    version.extend_from_slice(&2u32.to_be_bytes());
    version.extend_from_slice(&[0u8; 8]);
    version
}

fn mux_packet(proto: u32, tx: u16, payload: &[u8]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(16 + payload.len());
    pkt.extend_from_slice(&proto.to_be_bytes());
    pkt.extend_from_slice(&((16 + payload.len()) as u32).to_be_bytes());
    pkt.extend_from_slice(&MUX_MAGIC.to_be_bytes());
    pkt.extend_from_slice(&tx.to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes());
    pkt.extend_from_slice(payload);
    pkt
}

impl MuxHost {
    pub async fn open(serial: &str) -> Result<Arc<Self>, Error> {
        let stream = backend::open_stream(serial).await?;
        let (mut rd, mut wr) = tokio::io::split(stream);
        wr.write_all(&version_packet())
            .await
            .map_err(Error::io("usbmux version"))?;
        let mut reply = vec![0u8; READ_CHUNK];
        let _ = tokio::time::timeout(VERSION_WAIT, rd.read(&mut reply)).await;

        let (out, mut packets) = mpsc::channel::<(u32, Vec<u8>)>(QUEUED);
        let writer = tokio::spawn(async move {
            let mut tx = 0u16;
            while let Some((proto, payload)) = packets.recv().await {
                if wr
                    .write_all(&mux_packet(proto, tx, &payload))
                    .await
                    .is_err()
                {
                    return;
                }
                tx = tx.wrapping_add(1);
            }
        });
        let conns = Conns::default();
        let reader = tokio::spawn(read_packets(rd, out.clone(), conns.clone()));
        let _ = out.send((P_SETUP, vec![0x07])).await;
        Ok(Arc::new(Self {
            out,
            conns,
            next_sport: AtomicU16::new(1),
            tasks: Mutex::new(vec![writer, reader]),
        }))
    }

    pub fn shutdown(&self) {
        let conns: Vec<_> = self.conns.lock().unwrap().drain().map(|(_, c)| c).collect();
        for conn in conns {
            conn.close(&self.out);
        }
        for task in self.tasks.lock().unwrap().drain(..) {
            task.abort();
        }
    }

    pub async fn connect(self: &Arc<Self>, dport: u16) -> Result<MuxStream, Error> {
        let sport = self.next_sport.fetch_add(1, Ordering::SeqCst);
        let (up_tx, up) = oneshot::channel();
        let (rx_tx, rx) = mpsc::channel(QUEUED);
        let conn = Arc::new(Conn {
            sport,
            dport,
            state: Mutex::new(ConnState {
                tx_seq: 0,
                tx_ack: 0,
                closed: false,
                up: Some(up_tx),
                rx: Some(rx_tx),
            }),
        });
        self.conns.lock().unwrap().insert(sport, conn.clone());
        let syn = conn.segment(&conn.state.lock().unwrap(), TH_SYN, &[]);
        let sent = self.out.send((P_TCP, syn)).await.is_ok();
        if !sent || !matches!(tokio::time::timeout(CONNECT_WAIT, up).await, Ok(Ok(()))) {
            self.conns.lock().unwrap().remove(&sport);
            return Err(Error::ConnectFailed(dport));
        }
        Ok(MuxStream {
            host: self.clone(),
            conn,
            tx: PollSender::new(self.out.clone()),
            rx,
            leftover: Vec::new(),
            eof: false,
        })
    }
}

impl Drop for MuxHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn read_packets(mut rd: ReadHalf<UsbStream>, out: Out, conns: Conns) {
    let mut chunk = vec![0u8; READ_CHUNK];
    let mut rxbuf: Vec<u8> = Vec::new();
    'read: loop {
        let n = match rd.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        rxbuf.extend_from_slice(&chunk[..n]);
        while rxbuf.len() >= 8 {
            let proto = u32::from_be_bytes(rxbuf[0..4].try_into().unwrap());
            let length = u32::from_be_bytes(rxbuf[4..8].try_into().unwrap()) as usize;
            if !(8..=MAX_PACKET).contains(&length) {
                eprintln!("[usbmux] mux packet of {length} bytes, the stream is out of step");
                break 'read;
            }
            if rxbuf.len() < length {
                break;
            }
            let pkt: Vec<u8> = rxbuf.drain(..length).collect();
            if proto == P_TCP && length >= 36 {
                let dport = u16::from_be_bytes([pkt[18], pkt[19]]);
                let seq = u32::from_be_bytes(pkt[20..24].try_into().unwrap());
                let flags = pkt[29];
                let conn = conns.lock().unwrap().get(&dport).cloned();
                let Some(conn) = conn else {
                    continue;
                };
                let reaction = conn.on_packet(flags, seq, &pkt[36..length]);
                if let Some((rx, data)) = reaction.data {
                    let _ = rx.send(data).await;
                }
                for reply in reaction.replies {
                    if out.send((P_TCP, reply)).await.is_err() {
                        break 'read;
                    }
                }
            }
        }
    }
    let conns: Vec<_> = conns.lock().unwrap().drain().map(|(_, c)| c).collect();
    for conn in conns {
        end(&mut conn.state.lock().unwrap());
    }
}

pub struct MuxStream {
    host: Arc<MuxHost>,
    conn: Arc<Conn>,
    tx: PollSender<(u32, Vec<u8>)>,
    rx: mpsc::Receiver<Vec<u8>>,
    leftover: Vec<u8>,
    eof: bool,
}

impl fmt::Debug for MuxStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MuxStream")
            .field("sport", &self.conn.sport)
            .field("dport", &self.conn.dport)
            .finish()
    }
}

impl AsyncRead for MuxStream {
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
                Poll::Ready(Some(data)) => self.leftover = data,
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

impl AsyncWrite for MuxStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if let Err(e) = ready!(this.reserve(cx)) {
            return Poll::Ready(Err(e));
        }
        let chunk = &buf[..buf.len().min(MAX_PAYLOAD)];
        let segment = this.conn.data(chunk);
        Poll::Ready(
            this.tx
                .send_item((P_TCP, segment))
                .map(|()| chunk.len())
                .map_err(|_| closed()),
        )
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if ready!(this.reserve(cx)).is_err() {
            return Poll::Ready(Ok(()));
        }
        if let Some(fin) = this.conn.fin() {
            let _ = this.tx.send_item((P_TCP, fin));
        } else {
            this.tx.abort_send();
        }
        Poll::Ready(Ok(()))
    }
}

fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "mux closed")
}

impl MuxStream {
    fn reserve(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.tx.poll_reserve(cx).map_err(|_| closed())
    }
}

impl Drop for MuxStream {
    fn drop(&mut self) {
        self.conn.close(&self.host.out);
        self.host.conns.lock().unwrap().remove(&self.conn.sport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(rx: mpsc::Sender<Vec<u8>>) -> Conn {
        Conn {
            sport: 1,
            dport: 2,
            state: Mutex::new(ConnState {
                tx_seq: 10,
                tx_ack: 100,
                closed: false,
                up: None,
                rx: Some(rx),
            }),
        }
    }

    fn ack_of(segment: &[u8]) -> u32 {
        u32::from_be_bytes(segment[8..12].try_into().unwrap())
    }

    #[test]
    fn data_with_fin_hands_the_payload_over_and_acks_both() {
        let (tx, _rx) = mpsc::channel(1);
        let c = conn(tx);
        let r = c.on_packet(TH_ACK | TH_FIN, 100, &[1, 2, 3]);
        assert_eq!(r.data.map(|(_, d)| d), Some(vec![1, 2, 3]));
        let acks: Vec<u32> = r.replies.iter().map(|s| ack_of(s)).collect();
        assert_eq!(acks, [103, 104]);
        assert!(c.fin().is_none());
    }

    #[test]
    fn written_chunks_advance_the_sequence() {
        let (tx, _rx) = mpsc::channel(1);
        let c = conn(tx);
        let first = c.data(&[0; 5]);
        let second = c.data(&[0; 2]);
        let seq = |s: &[u8]| u32::from_be_bytes(s[4..8].try_into().unwrap());
        assert_eq!((seq(&first), seq(&second)), (10, 15));
        assert!(c.fin().is_some());
        assert!(c.fin().is_none());
    }
}
