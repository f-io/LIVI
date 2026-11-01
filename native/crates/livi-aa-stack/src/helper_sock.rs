// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::PathBuf;
use std::time::Duration;

pub use livi_session_io::line::Feed;
use livi_session_io::line::LineSock;
use serde_json::Value;
use tokio::sync::mpsc;

pub const SOCK_PATH: &str = "/tmp/aa-bt.sock";
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct AaHelperSock {
    sock: LineSock,
}

impl Default for AaHelperSock {
    fn default() -> Self {
        Self::new(SOCK_PATH)
    }
}

impl AaHelperSock {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            sock: LineSock::new(path, "aa sock"),
        }
    }

    pub async fn request(&self, line: &str) -> Result<Value, String> {
        self.sock.request(line, TIMEOUT).await
    }

    pub async fn set_wired_phones(&self, ids: &[String]) -> Result<Value, String> {
        self.request(&format!("wired-phones {}", Value::from(ids.to_vec())))
            .await
    }

    pub async fn set_sco_sink(&self, sink: Option<(&str, u32)>) -> Result<Value, String> {
        match sink {
            Some((feed, stream)) if !feed.is_empty() => {
                self.request(&format!("sco-sink {feed} {stream}")).await
            }
            _ => self.request("sco-sink").await,
        }
    }

    pub async fn restart_usb(&self) -> Result<Value, String> {
        self.request("restart-usb").await
    }

    pub fn subscribe(&self, retry: Duration) -> mpsc::UnboundedReceiver<Feed> {
        self.sock
            .subscribe(retry, |event| event.get("event").is_some())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    use super::*;

    pub(crate) struct Dir(pub PathBuf);

    impl Dir {
        pub(crate) fn new() -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("laa-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) fn serve(
        path: &Path,
        reply: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> Arc<Mutex<Vec<String>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let listener = UnixListener::bind(path).unwrap();
        let reply = Arc::new(reply);
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let reply = reply.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let (rd, mut wr) = sock.into_split();
                    let mut lines = BufReader::new(rd).lines();
                    if let Ok(Some(line)) = lines.next_line().await {
                        log.lock().unwrap().push(line.clone());
                        if let Some(answer) = reply(&line) {
                            let _ = wr.write_all(answer.as_bytes()).await;
                        }
                    }
                });
            }
        });
        seen
    }

    #[tokio::test]
    async fn requests_go_out_as_the_helper_reads_them() {
        let dir = Dir::new();
        let path = dir.0.join("aa.sock");
        let seen = serve(&path, |line| match line {
            "restart-usb" => Some("{\"ok\":true,\"count\":2}\n".into()),
            "sco-sink" => Some("{\"ok\":false,\"error\":\"no\"}\n".into()),
            l if l.starts_with("sco-sink ") => Some("not json\n".into()),
            _ => Some("{\"ok\":true}\n".into()),
        });
        let helper = AaHelperSock::new(&path);
        assert_eq!(
            helper.restart_usb().await,
            Ok(serde_json::json!({ "ok": true, "count": 2 }))
        );
        assert_eq!(helper.set_sco_sink(None).await.unwrap()["error"], "no");
        assert_eq!(
            helper.set_sco_sink(Some(("", 3))).await.unwrap()["ok"],
            false
        );
        assert!(
            helper
                .set_sco_sink(Some(("/f", 3)))
                .await
                .unwrap_err()
                .contains("bad json")
        );
        helper
            .set_wired_phones(&["a".into(), "b".into()])
            .await
            .unwrap();
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            [
                "restart-usb",
                "sco-sink",
                "sco-sink",
                "sco-sink /f 3",
                "wired-phones [\"a\",\"b\"]"
            ]
        );
        let missing = AaHelperSock::new(dir.0.join("none.sock"));
        assert!(
            missing
                .restart_usb()
                .await
                .unwrap_err()
                .starts_with("aa sock error")
        );
        assert_eq!(AaHelperSock::default().sock.path(), Path::new(SOCK_PATH));
    }

    #[tokio::test]
    async fn a_silent_or_short_answer_fails() {
        let dir = Dir::new();
        let path = dir.0.join("aa.sock");
        serve(&path, |line| {
            (line == "restart-usb").then(|| "{\"ok\":".to_string())
        });
        let helper = AaHelperSock::new(&path);
        assert_eq!(
            helper.restart_usb().await,
            Err("aa sock closed without response".into())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_hanging_helper_times_out() {
        let dir = Dir::new();
        let path = dir.0.join("aa.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let helper = AaHelperSock::new(&path);
        let ask = tokio::spawn(async move { helper.restart_usb().await });
        let (_held, _) = listener.accept().await.unwrap();
        assert_eq!(
            ask.await.unwrap(),
            Err("aa sock timeout after 5000ms".into())
        );
    }

    #[tokio::test]
    async fn the_subscription_comes_back_after_a_loss() {
        let dir = Dir::new();
        let path = dir.0.join("aa.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let helper = AaHelperSock::new(&path);
        let mut feed = helper.subscribe(Duration::from_millis(10));

        let (sock, _) = listener.accept().await.unwrap();
        let (rd, mut wr) = sock.into_split();
        let mut lines = BufReader::new(rd).lines();
        assert_eq!(
            lines.next_line().await.unwrap().as_deref(),
            Some("subscribe")
        );
        assert_eq!(feed.recv().await, Some(Feed::Connected));
        wr.write_all(b"{\"type\":\"x\"}\n\nnot json\n[1]\n{\"event\":\"sco\",\"up\":true}\n")
            .await
            .unwrap();
        assert_eq!(
            feed.recv().await,
            Some(Feed::Event(
                serde_json::json!({ "event": "sco", "up": true })
            ))
        );
        drop((lines, wr));

        let (again, _) = listener.accept().await.unwrap();
        assert_eq!(feed.recv().await, Some(Feed::Connected));
        drop(feed);
        drop(again);
    }
}
