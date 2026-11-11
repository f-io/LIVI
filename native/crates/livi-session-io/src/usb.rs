// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use nusb::descriptors::{ConfigurationDescriptor, TransferType};
use nusb::transfer::{Buffer, Bulk, Direction, In, Out, TransferError};
use nusb::{Device, Endpoint, Interface};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

const CLAIM_RETRIES: usize = 5;
const CLAIM_RETRY: Duration = Duration::from_millis(100);
const READ_LEN: usize = 16384;
const READS_IN_FLIGHT: usize = 4;
const WRITE_DRAIN: Duration = Duration::from_millis(250);
const QUEUED: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum UsbError {
    #[error("{what}: {source}")]
    Usb {
        what: &'static str,
        source: nusb::Error,
    },
    #[error("configuration: {0}")]
    Configuration(#[from] nusb::ActiveConfigurationError),
    #[error("no interface with bulk in and out")]
    NoBulkPair,
    #[error("claim interface {number}: {source}")]
    Claim { number: u8, source: nusb::Error },
}

fn usb(what: &'static str) -> impl FnOnce(nusb::Error) -> UsbError {
    move |source| UsbError::Usb { what, source }
}

pub struct Pipe {
    iface: Interface,
    ep_in: Endpoint<Bulk, In>,
    ep_out: Endpoint<Bulk, Out>,
    zlp: bool,
}

impl Pipe {
    pub fn with_zlp(mut self) -> Self {
        self.zlp = true;
        self
    }
}

pub async fn open_pipe(dev: &Device) -> Result<Pipe, UsbError> {
    let active = dev
        .active_configuration()
        .map(|c| c.configuration_value())
        .ok();
    if active != Some(1) {
        dev.set_configuration(1)
            .await
            .map_err(usb("set configuration"))?;
    }
    let (number, in_addr, out_addr) = {
        let cfg = dev.active_configuration()?;
        bulk_pair(&cfg).ok_or(UsbError::NoBulkPair)?
    };
    claim(dev, number, in_addr, out_addr, CLAIM_RETRIES, CLAIM_RETRY).await
}

pub async fn claim(
    dev: &Device,
    number: u8,
    in_addr: u8,
    out_addr: u8,
    tries: usize,
    wait: Duration,
) -> Result<Pipe, UsbError> {
    let mut tries = tries.max(1);
    loop {
        match dev.claim_interface(number).await {
            Ok(iface) => {
                let ep_in = iface
                    .endpoint::<Bulk, In>(in_addr)
                    .map_err(usb("bulk in"))?;
                let ep_out = iface
                    .endpoint::<Bulk, Out>(out_addr)
                    .map_err(usb("bulk out"))?;
                return Ok(Pipe {
                    iface,
                    ep_in,
                    ep_out,
                    zlp: false,
                });
            }
            Err(source) => {
                tries -= 1;
                if tries == 0 {
                    return Err(UsbError::Claim { number, source });
                }
                tokio::time::sleep(wait).await;
            }
        }
    }
}

fn bulk_pair(cfg: &ConfigurationDescriptor<'_>) -> Option<(u8, u8, u8)> {
    for intf in cfg.interfaces() {
        let Some(alt) = intf.alt_settings().next() else {
            continue;
        };
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

pub struct UsbStream {
    rx: mpsc::Receiver<Vec<u8>>,
    tx: Option<PollSender<Vec<u8>>>,
    leftover: Vec<u8>,
    eof: bool,
}

impl UsbStream {
    pub fn new(pipe: Pipe) -> Self {
        let (in_tx, in_rx) = mpsc::channel(QUEUED);
        let (out_tx, out_rx) = mpsc::channel(QUEUED);
        tokio::spawn(pump_in(pipe.ep_in, in_tx, pipe.iface));
        tokio::spawn(pump_out(pipe.ep_out, out_rx, pipe.zlp));
        Self {
            rx: in_rx,
            tx: Some(PollSender::new(out_tx)),
            leftover: Vec::new(),
            eof: false,
        }
    }
}

async fn pump_in(mut ep: Endpoint<Bulk, In>, tx: mpsc::Sender<Vec<u8>>, _iface: Interface) {
    for _ in 0..READS_IN_FLIGHT {
        let buf = ep.allocate(READ_LEN);
        ep.submit(buf);
    }
    loop {
        let done = tokio::select! {
            done = ep.next_complete() => done,
            _ = tx.closed() => break,
        };
        match done.status {
            Ok(()) => {
                if done.actual_len > 0
                    && tx
                        .send(done.buffer[..done.actual_len].to_vec())
                        .await
                        .is_err()
                {
                    break;
                }
            }
            Err(TransferError::Stall) => {
                if let Err(e) = ep.clear_halt().await {
                    eprintln!("[usb] clear halt: {e}");
                    break;
                }
            }
            Err(e) => {
                eprintln!("[usb] read: {e}");
                break;
            }
        }
        let buf = ep.allocate(READ_LEN);
        ep.submit(buf);
    }
}

async fn pump_out(mut ep: Endpoint<Bulk, Out>, mut rx: mpsc::Receiver<Vec<u8>>, zlp: bool) {
    while let Some(chunk) = rx.recv().await {
        let empty_after = zlp && ends_on_packet(chunk.len(), ep.max_packet_size());
        if !transfer(&mut ep, &rx, chunk).await
            || (empty_after && !transfer(&mut ep, &rx, Vec::new()).await)
        {
            break;
        }
    }
}

async fn transfer(
    ep: &mut Endpoint<Bulk, Out>,
    rx: &mpsc::Receiver<Vec<u8>>,
    chunk: Vec<u8>,
) -> bool {
    ep.submit(Buffer::from(chunk));
    let done = loop {
        match tokio::time::timeout(WRITE_DRAIN, ep.next_complete()).await {
            Ok(done) => break done,
            Err(_) if rx.is_closed() => return false,
            Err(_) => {}
        }
    };
    if let Err(e) = done.status {
        eprintln!("[usb] write: {e}");
        return false;
    }
    true
}

fn ends_on_packet(len: usize, max_packet: usize) -> bool {
    len != 0 && len.is_multiple_of(max_packet)
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
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        let broken = || io::Error::from(io::ErrorKind::BrokenPipe);
        let Some(tx) = self.get_mut().tx.as_mut() else {
            return Poll::Ready(Err(broken()));
        };
        match tx.poll_reserve(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(_)) => Poll::Ready(Err(broken())),
            Poll::Ready(Ok(())) => Poll::Ready(
                tx.send_item(data.to_vec())
                    .map(|()| data.len())
                    .map_err(|_| broken()),
            ),
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

    fn stream() -> (UsbStream, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        let (in_tx, in_rx) = mpsc::channel(QUEUED);
        let (out_tx, out_rx) = mpsc::channel(1);
        (
            UsbStream {
                rx: in_rx,
                tx: Some(PollSender::new(out_tx)),
                leftover: Vec::new(),
                eof: false,
            },
            in_tx,
            out_rx,
        )
    }

    #[test]
    fn only_a_transfer_that_fills_its_last_packet_needs_an_empty_one() {
        assert!(ends_on_packet(512, 512));
        assert!(ends_on_packet(1024, 512));
        assert!(!ends_on_packet(476, 512));
        assert!(!ends_on_packet(513, 512));
        assert!(!ends_on_packet(0, 512));
        assert!(!ends_on_packet(512, 0));
    }

    #[tokio::test]
    async fn reads_hand_out_a_chunk_across_short_reads_and_end_at_eof() {
        let (mut s, in_tx, _out) = stream();
        in_tx.send(vec![1, 2, 3, 4, 5]).await.unwrap();
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

    #[tokio::test]
    async fn a_full_queue_holds_the_writer_back_until_usb_drains_it() {
        let (mut s, _in, mut out) = stream();
        s.write_all(&[1]).await.unwrap();
        let mut second = std::pin::pin!(s.write_all(&[2]));
        assert!(futures_util::poll!(second.as_mut()).is_pending());
        assert_eq!(out.recv().await.unwrap(), vec![1]);
        second.await.unwrap();
        assert_eq!(out.recv().await.unwrap(), vec![2]);
    }
}
