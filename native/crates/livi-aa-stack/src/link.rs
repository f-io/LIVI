// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::path::Path;
use std::sync::Arc;

use livi_session_io::link::{Framer, Item, encode_control, encode_message};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;
use tokio::sync::{Notify, mpsc};

const QUEUED: usize = 1024;

use crate::channels::Frame;

enum Write {
    Bytes(Vec<u8>),
    Shutdown,
}

pub struct Link {
    pub peer: String,
    tx: mpsc::Sender<Write>,
    stalled: Arc<Notify>,
    writable: bool,
    closed: bool,
}

pub struct LinkReader {
    rd: OwnedReadHalf,
    framer: Framer,
    buf: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkItem {
    Message(Frame),
    Control(Value),
    Closed(Option<String>),
}

pub async fn connect(path: impl AsRef<Path>, peer: &str) -> io::Result<(Link, LinkReader)> {
    Ok(attach(UnixStream::connect(path).await?, peer))
}

pub fn attach(sock: UnixStream, peer: &str) -> (Link, LinkReader) {
    let (rd, mut wr) = sock.into_split();
    let (tx, mut rx) = mpsc::channel::<Write>(QUEUED);
    let stalled = Arc::new(Notify::new());
    let stop = stalled.clone();
    tokio::spawn(async move {
        let writes = async {
            while let Some(w) = rx.recv().await {
                let result = match w {
                    Write::Bytes(bytes) => wr.write_all(&bytes).await,
                    Write::Shutdown => wr.shutdown().await,
                };
                if result.is_err() {
                    return;
                }
            }
        };
        tokio::select! {
            () = writes => {}
            () = stop.notified() => {}
        }
    });
    let link = Link {
        peer: peer.to_string(),
        tx,
        stalled,
        writable: true,
        closed: false,
    };
    (
        link,
        LinkReader {
            rd,
            framer: Framer::default(),
            buf: vec![0; 65536],
        },
    )
}

impl Link {
    fn write(&self, bytes: Vec<u8>) {
        if self.closed || !self.writable {
            return;
        }
        self.queue(Write::Bytes(bytes));
    }

    fn queue(&self, w: Write) {
        if let Err(mpsc::error::TrySendError::Full(_)) = self.tx.try_send(w) {
            eprintln!("[aa] {} takes nothing more, the link ends", self.peer);
            self.stalled.notify_one();
        }
    }

    pub fn send(&self, frame: &Frame) {
        self.write(encode_message(
            frame.ch,
            frame.flags,
            frame.msg_id,
            &frame.payload,
        ));
    }

    pub fn control(&self, msg: &Value) {
        self.write(encode_control(&msg.to_string()));
    }

    pub fn end(&self) {
        self.control(&serde_json::json!({ "type": "end" }));
    }

    pub fn destroy(&mut self) {
        if self.closed {
            return;
        }
        self.control(&serde_json::json!({ "type": "close" }));
        if self.writable {
            self.writable = false;
            self.queue(Write::Shutdown);
        }
    }

    pub fn mark_closed(&mut self) {
        self.closed = true;
    }
}

impl LinkReader {
    pub async fn next(&mut self) -> LinkItem {
        loop {
            while let Some(item) = self.framer.next_item() {
                match item {
                    Err(e) => return LinkItem::Closed(Some(e.to_string())),
                    Ok(Item::Message {
                        ch,
                        flags,
                        msg_id,
                        payload,
                    }) => {
                        return LinkItem::Message(Frame {
                            ch,
                            flags,
                            msg_id,
                            payload,
                        });
                    }
                    Ok(Item::Control(json)) => {
                        if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(&json)
                            && v.get("type").is_some_and(Value::is_string)
                        {
                            return LinkItem::Control(v);
                        }
                    }
                }
            }
            match self.rd.read(&mut self.buf).await {
                Ok(0) => return LinkItem::Closed(None),
                Ok(n) => self.framer.push(&self.buf[..n]),
                Err(e) => return LinkItem::Closed(Some(e.to_string())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn items_cross_both_ways() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (mut link, mut reader) = attach(ours, "10.0.0.2");
        let (mut helper, mut helper_rd) = attach(theirs, "");

        link.send(&Frame::new(3, 0x0b, 0x8004, [8, 1]));
        link.control(&serde_json::json!({ "type": "sink", "feed": "/f" }));
        assert_eq!(
            helper_rd.next().await,
            LinkItem::Message(Frame::new(3, 0x0b, 0x8004, [8, 1]))
        );
        assert_eq!(
            helper_rd.next().await,
            LinkItem::Control(serde_json::json!({ "type": "sink", "feed": "/f" }))
        );

        helper.write(encode_control("not json"));
        helper.write(encode_control("{\"type\":7}"));
        helper.write(encode_control("[1]"));
        helper.control(&serde_json::json!({ "type": "ready", "mic": "/m" }));
        assert_eq!(
            reader.next().await,
            LinkItem::Control(serde_json::json!({ "type": "ready", "mic": "/m" }))
        );

        link.end();
        assert_eq!(
            helper_rd.next().await,
            LinkItem::Control(serde_json::json!({ "type": "end" }))
        );
        link.destroy();
        link.send(&Frame::new(0, 3, 1, []));
        assert_eq!(
            helper_rd.next().await,
            LinkItem::Control(serde_json::json!({ "type": "close" }))
        );
        assert_eq!(helper_rd.next().await, LinkItem::Closed(None));
        helper.destroy();
        drop(helper);
        assert_eq!(
            reader.next().await,
            LinkItem::Control(serde_json::json!({ "type": "close" }))
        );
        assert_eq!(reader.next().await, LinkItem::Closed(None));
        link.mark_closed();
        link.destroy();
    }

    #[tokio::test]
    async fn a_peer_that_stops_reading_gets_the_link_closed() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        let (link, _reader) = attach(ours, "stuck");
        for _ in 0..QUEUED + 64 {
            link.send(&Frame::new(3, 0x0b, 0x8004, vec![0; 16384]));
            tokio::task::yield_now().await;
        }
        let mut sink = Vec::new();
        let drained = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            theirs.read_to_end(&mut sink),
        )
        .await;
        assert!(matches!(drained, Ok(Ok(_))));
    }

    #[tokio::test]
    async fn a_missing_socket_fails_the_connect() {
        assert!(connect("/nonexistent/aa-session.sock", "x").await.is_err());
    }
}
