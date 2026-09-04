// Streams media records into gst-host over its feed socket.

use tokio::io::AsyncWriteExt;
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
            let mut sock = match UnixStream::connect(&path).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[aa-feed] connect {path}: {e}");
                    return;
                }
            };
            while let Some(record) = rx.recv().await {
                if let Err(e) = sock.write_all(&record).await {
                    eprintln!("[aa-feed] write: {e}");
                    return;
                }
            }
        });
        Self { tx }
    }

    /// False when the writer is behind or gone, the record is dropped then.
    pub fn send(&self, record: Vec<u8>) -> bool {
        self.tx.try_send(record).is_ok()
    }
}
