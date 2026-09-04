// A USB bulk in/out pair as an async byte stream for a session, pumped by two
// tasks. The transport-specific bring-up (AOAP for Android Auto, the dongle's
// hotplug watcher) lives with each driver; opening the pipe and moving bytes is
// the same for both.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use nusb::descriptors::{ConfigurationDescriptor, TransferType};
use nusb::transfer::{Buffer, Bulk, Direction, In, Out, TransferError};
use nusb::{Device, Endpoint, Interface};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

const CLAIM_RETRIES: usize = 5;
const CLAIM_RETRY: Duration = Duration::from_millis(100);
const READ_LEN: usize = 16384;
const READS_IN_FLIGHT: usize = 4;

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

async fn pump_out(mut ep: Endpoint<Bulk, Out>, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(chunk) = rx.recv().await {
        ep.submit(Buffer::from(chunk));
        let done = ep.next_complete().await;
        if let Err(e) = done.status {
            eprintln!("[usb] write: {e}");
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
}
