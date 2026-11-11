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
use livi_core_proto::frame::{Decoder, FrameError, encode};
use livi_core_proto::input::Input;
use livi_core_proto::message::{Action, ToCore};
use livi_core_proto::patch::{PatchOp, apply};
use livi_core_proto::state::{Front, Screen, State};
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum Event {
    Welcome { version: String },
    Changed(BTreeSet<String>),
    Spectrum(Vec<f32>),
    OpenPage(String),
    Gone(String),
}

#[derive(Debug, Default)]
pub struct Mirror {
    pub json: Value,
    pub state: State,
}

impl Mirror {
    pub fn new(json: Value) -> Mirror {
        let mut mirror = Mirror {
            json,
            state: State::default(),
        };
        let sections: Vec<String> = mirror
            .json
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        sections.iter().for_each(|s| mirror.refresh(s));
        mirror
    }

    fn patch(&mut self, ops: &[PatchOp]) -> BTreeSet<String> {
        let mut sections = BTreeSet::new();
        for op in ops {
            let (PatchOp::Set { path, .. } | PatchOp::Remove { path }) = op;
            sections.insert(path.first().cloned().unwrap_or_default());
            let _ = apply(&mut self.json, op);
        }
        if sections.contains("") {
            *self = Mirror::new(std::mem::take(&mut self.json));
        } else {
            sections.iter().for_each(|s| self.refresh(s));
        }
        sections
    }

    fn refresh(&mut self, section: &str) {
        let value = self.json.get(section).unwrap_or(&Value::Null);
        if let Err(e) = self.state.refresh(section, value) {
            eprintln!("[core-client] {section}: {e}");
        }
    }
}

#[derive(Clone)]
pub struct Client {
    stream: Arc<Mutex<UnixStream>>,
    mirror: Arc<Mutex<Mirror>>,
    next_id: Arc<AtomicU32>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Client")
    }
}

pub fn socket_path() -> PathBuf {
    // SAFETY: getuid has no preconditions.
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
        on_event: impl FnMut(Event, &Mutex<Mirror>) + Send + 'static,
    ) -> io::Result<Client> {
        let reader = stream.try_clone()?;
        let client = Client {
            stream: Arc::new(Mutex::new(stream)),
            mirror: Arc::new(Mutex::new(Mirror::default())),
            next_id: Arc::new(AtomicU32::new(1)),
        };
        client.send(&ToCore::Hello {
            protocol: PROTOCOL,
            client: name.into(),
        });
        let mirror = client.mirror.clone();
        thread::Builder::new()
            .name("core-client".into())
            .spawn(move || read_loop(reader, &mirror, on_event))?;
        Ok(client)
    }

    pub fn mirror(&self) -> MutexGuard<'_, Mirror> {
        self.mirror.lock().unwrap_or_else(|e| e.into_inner())
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
    mirror: &Mutex<Mirror>,
    mut on_event: impl FnMut(Event, &Mutex<Mirror>),
) {
    let mut emit = |ev: Event| on_event(ev, mirror);
    let lock = || mirror.lock().unwrap_or_else(|e| e.into_inner());
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
            let mut msg = match msg {
                Ok(msg) => msg,
                Err(FrameError::Json(_)) => continue,
                Err(e @ FrameError::TooLarge(_)) => return emit(Event::Gone(e.to_string())),
            };
            match msg["type"].as_str() {
                Some("welcome") => {
                    *lock() = Mirror::new(msg["state"].take());
                    let version = msg["version"].as_str().unwrap_or("").to_string();
                    emit(Event::Welcome { version });
                }
                Some("patch") => {
                    let ops: Vec<PatchOp> =
                        serde_json::from_value(msg["ops"].take()).unwrap_or_default();
                    let sections = lock().patch(&ops);
                    emit(Event::Changed(sections));
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

#[cfg(test)]
mod tests {
    use livi_core_proto::state::Protocol;
    use serde_json::json;

    use super::*;

    fn set(path: &[&str], value: Value) -> PatchOp {
        PatchOp::Set {
            path: path.iter().map(|p| p.to_string()).collect(),
            value,
        }
    }

    #[test]
    fn a_patch_reads_the_touched_sections_again() {
        let mut mirror = Mirror::new(State::filled(json!({})));
        let sections = mirror.patch(&[
            set(&["sessions", "active"], json!("carplay")),
            set(&["config", "language"], json!("de")),
        ]);
        assert_eq!(
            sections,
            BTreeSet::from(["config".to_string(), "sessions".to_string()])
        );
        assert_eq!(mirror.state.sessions.active, Some(Protocol::Carplay));
        assert_eq!(mirror.state.config.language, "de");
        assert_eq!(mirror.json["config"]["language"], "de");
    }

    #[test]
    fn a_new_root_reads_every_section() {
        let mut mirror = Mirror::default();
        mirror.patch(&[set(
            &[],
            State::filled(json!({ "sessions": { "total": 2 } })),
        )]);
        assert_eq!(mirror.state.sessions.total, 2);
    }

    #[test]
    fn a_broken_section_keeps_what_was_read_before() {
        let mut mirror = Mirror::new(State::filled(json!({ "sessions": { "total": 2 } })));
        mirror.patch(&[set(&["sessions", "total"], json!("many"))]);
        assert_eq!(mirror.state.sessions.total, 2);
    }
}
