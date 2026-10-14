use std::sync::{Arc, Mutex, MutexGuard};

use livi_core_proto::PROTOCOL;
use livi_core_proto::config::Config;
use livi_core_proto::frame::encode;
use livi_core_proto::message::FromCore;
use livi_core_proto::patch::diff;
use livi_core_proto::state::State;
use serde_json::Value;
use tokio::sync::{broadcast, watch};

/// A client further behind than this gets the whole state again.
const BACKLOG: usize = 256;

pub type Frame = Arc<Vec<u8>>;

pub struct Hub {
    inner: Mutex<Inner>,
    patches: broadcast::Sender<Frame>,
    current: watch::Sender<State>,
    applied: watch::Sender<Config>,
}

struct Inner {
    state: State,
    value: Value,
    rev: u64,
}

impl Hub {
    pub fn new(state: State) -> Self {
        let value = serde_json::to_value(&state).unwrap_or(Value::Null);
        let (patches, _) = broadcast::channel(BACKLOG);
        let (current, _) = watch::channel(state.clone());
        let (applied, _) = watch::channel(state.config.clone());
        Self { inner: Mutex::new(Inner { state, value, rev: 0 }), patches, current, applied }
    }

    pub fn watch(&self) -> watch::Receiver<State> {
        self.current.subscribe()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Taken under one lock, so no patch falls between the state and the subscription.
    pub fn welcome(&self) -> (FromCore, broadcast::Receiver<Frame>) {
        let inner = self.lock();
        let msg = FromCore::Welcome {
            protocol: PROTOCOL,
            version: env!("CARGO_PKG_VERSION").into(),
            rev: inner.rev,
            state: Box::new(inner.state.clone()),
        };
        (msg, self.patches.subscribe())
    }

    pub fn config(&self) -> Config {
        self.lock().state.config.clone()
    }

    pub fn applied(&self) -> watch::Receiver<Config> {
        self.applied.subscribe()
    }

    pub fn apply_pending(&self) -> bool {
        *self.applied.borrow() != self.config()
    }

    pub fn apply(&self) -> Option<(Config, Config)> {
        let next = self.config();
        let before = self.applied.borrow().clone();
        if before == next {
            return None;
        }
        self.applied.send_replace(next.clone());
        Some((before, next))
    }

    pub fn update(&self, change: impl FnOnce(&mut State)) {
        let mut inner = self.lock();
        change(&mut inner.state);
        let value = serde_json::to_value(&inner.state).unwrap_or(Value::Null);
        let ops = diff(&inner.value, &value);
        if ops.is_empty() {
            return;
        }
        inner.rev += 1;
        inner.value = value;
        self.current.send_replace(inner.state.clone());
        match encode(&FromCore::Patch { rev: inner.rev, ops }) {
            Ok(frame) => {
                let _ = self.patches.send(Arc::new(frame));
            }
            Err(e) => eprintln!("[core] patch {} not sent: {e}", inner.rev),
        }
    }
}

#[cfg(test)]
mod tests {
    use livi_core_proto::frame::Decoder;
    use livi_core_proto::patch::PatchOp;
    use livi_core_proto::state::{Front, PerScreen};
    use serde_json::json;

    use super::*;
    use crate::config_file::defaults;

    fn hub() -> Hub {
        let livi = PerScreen { main: Front::Livi, cluster: Front::Livi, aux: Front::Livi };
        Hub::new(State {
            front: livi,
            sessions: Default::default(),
            now_playing: Default::default(),
            telemetry: Default::default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            config: defaults(),
        })
    }

    fn decode(frame: &[u8]) -> FromCore {
        let mut dec = Decoder::new();
        dec.push(frame);
        dec.next_message().unwrap().unwrap()
    }

    #[test]
    fn a_change_goes_out_as_the_next_revision() {
        let hub = hub();
        let (welcome, mut patches) = hub.welcome();
        assert!(matches!(welcome, FromCore::Welcome { rev: 0, .. }));

        hub.update(|s| s.front.main = Front::Projection);
        let patch = decode(&patches.try_recv().unwrap());
        assert_eq!(
            patch,
            FromCore::Patch {
                rev: 1,
                ops: vec![PatchOp::Set {
                    path: vec!["front".into(), "main".into()],
                    value: json!("projection")
                }]
            }
        );
        let (later, _) = hub.welcome();
        assert!(matches!(later, FromCore::Welcome { rev: 1, .. }));
    }

    #[test]
    fn no_change_sends_nothing() {
        let hub = hub();
        let (_, mut patches) = hub.welcome();
        hub.update(|s| s.front.main = Front::Livi);
        assert!(patches.try_recv().is_err());
    }

    #[test]
    fn config_reads_the_current_state() {
        let hub = hub();
        hub.update(|s| s.config.hu_volume = 0.25);
        assert_eq!(hub.config().hu_volume, 0.25);
    }
}
