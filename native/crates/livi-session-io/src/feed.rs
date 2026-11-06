// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

const QUEUE: usize = 512;

pub struct FeedWriter {
    tx: mpsc::Sender<Vec<u8>>,
}

impl FeedWriter {
    pub fn open(path: String) -> Self {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(QUEUE);
        tokio::spawn(async move {
            let sock = match UnixStream::connect(&path).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[aa-feed] connect {path}: {e}");
                    return;
                }
            };
            let (mut rd, mut wr) = sock.into_split();
            let mut probe = [0u8; 1];
            loop {
                tokio::select! {
                    record = rx.recv() => {
                        let Some(record) = record else { return };
                        if let Err(e) = wr.write_all(&record).await {
                            eprintln!("[aa-feed] write: {e}");
                            return;
                        }
                    }
                    _ = rd.read(&mut probe) => {
                        eprintln!("[aa-feed] the host closed {path}");
                        return;
                    }
                }
            }
        });
        Self { tx }
    }

    pub fn send(&self, record: Vec<u8>) -> bool {
        self.tx.try_send(record).is_ok()
    }

    pub fn closed(&self) -> bool {
        self.tx.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixListener;

    #[tokio::test]
    async fn the_writer_closes_when_the_host_goes() {
        let path = std::env::temp_dir().join(format!("livi-feed-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let writer = FeedWriter::open(path.display().to_string());
        let (mut host, _) = listener.accept().await.unwrap();
        assert!(writer.send(b"rec".to_vec()));
        let mut got = [0u8; 3];
        host.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"rec");
        assert!(!writer.closed());
        drop(host);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !writer.closed() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
