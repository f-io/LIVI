// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use livi_core_proto::PROTOCOL;
use livi_core_proto::frame::{Decoder, encode};
use livi_core_proto::input::Input;
use livi_core_proto::message::{Action, ToCore};
use livi_core_proto::patch::{PatchOp, apply};
use livi_core_proto::state::{Front, Screen};
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum Event {
    Welcome { version: String },
    Changed(BTreeSet<String>),
    Spectrum(Vec<f32>),
    OpenPage(String),
    Gone(String),
}

#[derive(Clone)]
pub struct Client {
    stream: Arc<Mutex<UnixStream>>,
    state: Arc<Mutex<Value>>,
    next_id: Arc<AtomicU32>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Client")
    }
}

pub fn socket_path() -> PathBuf {
    livi_core_proto::runtime::core_socket(unsafe { libc::getuid() })
}

pub fn wait_for_socket(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    let until = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() >= until => return Err(e),
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

impl Client {
    pub fn start(
        stream: UnixStream,
        name: &str,
        on_event: impl FnMut(Event, &Value) + Send + 'static,
    ) -> io::Result<Client> {
        let reader = stream.try_clone()?;
        let client = Client {
            stream: Arc::new(Mutex::new(stream)),
            state: Arc::new(Mutex::new(Value::Null)),
            next_id: Arc::new(AtomicU32::new(1)),
        };
        client.send(&ToCore::Hello {
            protocol: PROTOCOL,
            client: name.into(),
        });
        let state = client.state.clone();
        thread::Builder::new()
            .name("core-client".into())
            .spawn(move || read_loop(reader, &state, on_event))?;
        Ok(client)
    }

    pub fn state(&self) -> MutexGuard<'_, Value> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_value(self.state().get(key)?.clone()).ok()
    }

    pub fn send(&self, msg: &ToCore) {
        let Ok(bytes) = encode(msg) else { return };
        let mut s = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        let _ = s.write_all(&bytes);
    }

    pub fn act(&self, action: Action) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.send(&ToCore::Action { id, action });
    }

    pub fn input(&self, input: Input) {
        self.send(&ToCore::Input { input });
    }

    pub fn path(&self, path: &str) {
        self.send(&ToCore::Path { path: path.into() });
    }

    pub fn show(&self, screen: Screen, front: Front) {
        self.act(Action::Show { screen, front });
    }

    pub fn spectrum(&self, on: bool) {
        self.send(&ToCore::Spectrum { on });
    }
}

fn read_loop(
    mut stream: UnixStream,
    state: &Mutex<Value>,
    mut on_event: impl FnMut(Event, &Value),
) {
    let mut emit = |ev: Event| {
        let s = state.lock().unwrap_or_else(|e| e.into_inner());
        on_event(ev, &s);
    };
    let mut dec = Decoder::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => return emit(Event::Gone("core closed the connection".into())),
            Ok(n) => n,
            Err(e) => return emit(Event::Gone(e.to_string())),
        };
        dec.push(&buf[..n]);
        while let Some(msg) = dec.next_message::<Value>() {
            let Ok(mut msg) = msg else { continue };
            match msg["type"].as_str() {
                Some("welcome") => {
                    *state.lock().unwrap_or_else(|e| e.into_inner()) = msg["state"].take();
                    let version = msg["version"].as_str().unwrap_or("").to_string();
                    emit(Event::Welcome { version });
                }
                Some("patch") => {
                    let ops: Vec<PatchOp> =
                        serde_json::from_value(msg["ops"].take()).unwrap_or_default();
                    let mut keys = BTreeSet::new();
                    {
                        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                        for op in &ops {
                            let (PatchOp::Set { path, .. } | PatchOp::Remove { path }) = op;
                            keys.insert(path.first().cloned().unwrap_or_default());
                            let _ = apply(&mut s, op);
                        }
                    }
                    emit(Event::Changed(keys));
                }
                Some("spectrum") => {
                    let bands = msg["bands"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_f64)
                                .map(|v| v as f32)
                                .collect()
                        })
                        .unwrap_or_default();
                    emit(Event::Spectrum(bands));
                }
                Some("openPage") => {
                    emit(Event::OpenPage(msg["path"].as_str().unwrap_or("").into()));
                }
                Some("refused") => {
                    return emit(Event::Gone(msg["reason"].as_str().unwrap_or("").into()));
                }
                _ => {}
            }
        }
    }
}
