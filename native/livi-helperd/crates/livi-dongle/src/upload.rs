//! Bridges a stock CarlinKit dongle's USB bulk pipe to a unix socket so the main process can
//! upload files to it (the server.cgi bootstrap, then any file). The main process speaks the
//! CarlinKit wire itself; this frames only the node->dongle direction, so each message is one
//! bulk transfer, and forwards the dongle->node direction as it arrives.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::Notify;

use crate::wire;

const NODE_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_CHUNK: usize = 65536;

/// Serves one upload session over `io` until the node disconnects or the dongle is unplugged.
pub async fn run<T>(io: T, label: &str, node: UnixListener, cancel: Arc<Notify>)
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let node_stream = match tokio::time::timeout(NODE_ACCEPT_TIMEOUT, node.accept()).await {
        Ok(Ok((s, _))) => s,
        _ => {
            eprintln!("[dongle] {label}: main process did not attach for upload");
            return;
        }
    };
    drop(node);
    let (mut node_rd, mut node_wr) = node_stream.into_split();
    let (mut rd, mut wr) = tokio::io::split(io);

    // dongle -> node: forward bulk reads as they arrive.
    let up = tokio::spawn(async move {
        let mut buf = vec![0u8; READ_CHUNK];
        loop {
            match rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) if node_wr.write_all(&buf[..n]).await.is_err() => break,
                Ok(_) => {}
            }
        }
    });

    // node -> dongle: one whole CarlinKit message per bulk write.
    let down = async move {
        let mut acc: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; READ_CHUNK];
        loop {
            let n = match node_rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            acc.extend_from_slice(&buf[..n]);
            while acc.len() >= wire::HEADER_LEN {
                let head: [u8; wire::HEADER_LEN] = acc[..wire::HEADER_LEN].try_into().unwrap();
                let (_, len) = match wire::parse_header(&head) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[dongle] {label}: upload framing: {e}");
                        return;
                    }
                };
                if acc.len() < wire::HEADER_LEN + len {
                    break;
                }
                let msg: Vec<u8> = acc.drain(..wire::HEADER_LEN + len).collect();
                if wr.write_all(&msg).await.is_err() {
                    return;
                }
            }
        }
    };

    tokio::select! {
        _ = down => {}
        _ = cancel.notified() => {}
    }
    up.abort();
}
