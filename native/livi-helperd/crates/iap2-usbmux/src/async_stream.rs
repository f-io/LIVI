// Async byte-stream view of a mux TCP connection, for the lockdown/TLS client.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use crate::mux::MuxTcpConn;

#[derive(Debug)]
pub struct AsyncMuxStream {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    leftover: Vec<u8>,
    eof: bool,
}

impl AsyncMuxStream {
    pub fn new(conn: Arc<MuxTcpConn>) -> Self {
        let (rx_tx, rx) = mpsc::unbounded_channel();
        let (tx, mut tx_rx) = mpsc::unbounded_channel::<Vec<u8>>();

        let reader_conn = conn.clone();
        std::thread::spawn(move || loop {
            match reader_conn.recv(std::time::Duration::from_secs(30)) {
                Some(data) if !data.is_empty() => {
                    if rx_tx.send(data).is_err() {
                        return;
                    }
                }
                Some(_) => return,
                None => {
                    if rx_tx.is_closed() {
                        return;
                    }
                }
            }
        });

        tokio::spawn(async move {
            while let Some(data) = tx_rx.recv().await {
                let conn = conn.clone();
                if tokio::task::spawn_blocking(move || conn.send(&data)).await.is_err() {
                    return;
                }
            }
        });

        Self { rx, tx, leftover: Vec::new(), eof: false }
    }
}

impl AsyncRead for AsyncMuxStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.leftover.is_empty() {
            let n = self.leftover.len().min(buf.remaining());
            let rest = self.leftover.split_off(n);
            buf.put_slice(&self.leftover);
            self.leftover = rest;
            return Poll::Ready(Ok(()));
        }
        if self.eof {
            return Poll::Ready(Ok(()));
        }
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(data)) => {
                let n = data.len().min(buf.remaining());
                buf.put_slice(&data[..n]);
                if n < data.len() {
                    self.leftover = data[n..].to_vec();
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => {
                self.eof = true;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for AsyncMuxStream {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.tx.send(buf.to_vec()) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "mux closed"))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
