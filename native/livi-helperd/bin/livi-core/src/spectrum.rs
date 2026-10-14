use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::frame::encode;
use livi_core_proto::message::FromCore;
use livi_core_proto::state::State;
use livi_media::gst_host::{GstHost, HostEvent};
use tokio::sync::{broadcast, watch};
use tokio::time::Instant;

use crate::hub::Frame;

/// A client that falls behind skips frames, a spectrum has no history.
const BUFFERED: usize = 8;

pub struct Feed {
    frames: broadcast::Sender<Frame>,
    viewers: watch::Sender<usize>,
}

pub struct Viewer {
    frames: broadcast::Receiver<Frame>,
    feed: Arc<Feed>,
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.feed.viewers.send_modify(|n| *n = n.saturating_sub(1));
    }
}

impl Viewer {
    pub async fn next(&mut self) -> Frame {
        loop {
            match self.frames.recv().await {
                Ok(frame) => return frame,
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                // The feed outlives every viewer, so this never comes.
                Err(broadcast::error::RecvError::Closed) => std::future::pending().await,
            }
        }
    }
}

impl Feed {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { frames: broadcast::channel(BUFFERED).0, viewers: watch::channel(0).0 })
    }

    pub fn watch(self: &Arc<Self>) -> Viewer {
        self.viewers.send_modify(|n| *n += 1);
        Viewer { frames: self.frames.subscribe(), feed: self.clone() }
    }

    #[cfg(test)]
    pub fn viewing(&self) -> usize {
        *self.viewers.borrow()
    }

    pub fn publish(&self, bands: Vec<f32>) {
        if self.frames.receiver_count() == 0 {
            return;
        }
        match encode(&FromCore::Spectrum { bands }) {
            Ok(frame) => {
                let _ = self.frames.send(Arc::new(frame));
            }
            Err(e) => eprintln!("[core] spectrum frame not sent: {e}"),
        }
    }
}

async fn until(due: Option<Instant>) {
    match due {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

pub async fn follow(gst: GstHost, feed: Arc<Feed>, state: watch::Receiver<State>) {
    let mut viewers = feed.viewers.subscribe();
    let mut events = gst.subscribe();
    let mut queue: VecDeque<(Instant, Vec<f32>)> = VecDeque::new();
    let mut tapping = false;
    loop {
        let wanted = *viewers.borrow_and_update() > 0;
        if wanted != tapping {
            gst.set_visualizer_tap(wanted);
            tapping = wanted;
            queue.clear();
        }
        let due = queue.front().map(|(at, _)| *at);
        tokio::select! {
            changed = viewers.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            event = events.recv() => match event {
                Ok(HostEvent::Spectrum { bands }) if tapping => {
                    let delay = state.borrow().config.visual_audio_delay_ms;
                    queue.push_back((Instant::now() + Duration::from_millis(delay.into()), bands));
                }
                Err(broadcast::error::RecvError::Closed) => return,
                _ => {}
            },
            () = until(due) => {
                if let Some((_, bands)) = queue.pop_front() {
                    feed.publish(bands);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use livi_core_proto::frame::Decoder;
    use livi_core_proto::state::{Front, PerScreen};
    use livi_host_proto::{Framer, encode_reply};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    use super::*;
    use crate::config_file::defaults;
    use crate::config_file::tests::TempDir;

    const OP_VISUALIZER: u8 = 15;
    const REPLY_VISUALIZER: u8 = 6;

    struct FakeHost {
        stream: UnixStream,
        framer: Framer,
    }

    impl FakeHost {
        async fn connect(sock: &std::path::Path) -> Self {
            for _ in 0..200 {
                if let Ok(stream) = UnixStream::connect(sock).await {
                    return Self { stream, framer: Framer::new() };
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("core never opened the gst-host socket");
        }

        async fn next(&mut self) -> (u8, Vec<u8>) {
            let mut buf = [0u8; 256];
            loop {
                if let Some(m) = self.framer.next_message() {
                    return (m.op, m.rest);
                }
                let n = tokio::time::timeout(Duration::from_secs(5), self.stream.read(&mut buf))
                    .await
                    .unwrap()
                    .unwrap();
                assert!(n > 0, "core closed the socket");
                self.framer.push(&buf[..n]);
            }
        }
    }

    fn state(delay_ms: u32) -> State {
        let mut config = defaults();
        config.visual_audio_delay_ms = delay_ms;
        State {
            front: PerScreen { main: Front::Livi, cluster: Front::Livi, aux: Front::Livi },
            sessions: Default::default(),
            now_playing: Default::default(),
            telemetry: Default::default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            config,
        }
    }

    fn bands_of(frame: &Frame) -> Vec<f32> {
        let mut dec = Decoder::new();
        dec.push(frame);
        match dec.next_message::<FromCore>() {
            Some(Ok(FromCore::Spectrum { bands })) => bands,
            other => panic!("not a spectrum frame: {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_tap_follows_the_viewers_and_frames_wait_out_the_delay() {
        let dir = TempDir::new();
        let sock = dir.0.join("gst.sock");
        let gst = GstHost::new(sock.clone());
        let feed = Feed::new();
        let (_state_tx, state_rx) = watch::channel(state(50));
        tokio::spawn(follow(gst.clone(), feed.clone(), state_rx));

        let mut first = feed.watch();
        let second = feed.watch();
        let mut host = FakeHost::connect(&sock).await;
        assert_eq!(host.next().await, (OP_VISUALIZER, vec![1]));

        let sent = Instant::now();
        let bands: Vec<u8> = [0.5f32, 0.75].iter().flat_map(|b| b.to_le_bytes()).collect();
        host.stream.write_all(&encode_reply(REPLY_VISUALIZER, 0, &bands)).await.unwrap();
        assert_eq!(bands_of(&first.next().await), [0.5, 0.75]);
        assert!(sent.elapsed() >= Duration::from_millis(50));

        drop(second);
        drop(first);
        assert_eq!(host.next().await, (OP_VISUALIZER, vec![0]));
        assert_eq!(*feed.viewers.borrow(), 0);
    }
}
