// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use livi_core_proto::LIFELINE_ENV;

pub const RELAUNCH: i32 = 75;

pub struct CoreProcess {
    pid: i32,
    ended: mpsc::Receiver<Option<i32>>,
}

impl CoreProcess {
    pub fn start(
        planes: &Path,
        on_exit: impl FnOnce(Option<i32>) + Send + 'static,
    ) -> io::Result<Self> {
        let bundle = crate::bundle::find();
        let bin = match &bundle {
            Some(b) => b.core(),
            None => std::env::current_exe()?.with_file_name("livi-core"),
        };
        println!("[ui] starting {}", bin.display());
        let mut cmd = Command::new(&bin);
        if let Some(b) = &bundle {
            cmd.env("LIVI_RESOURCES", &b.resources);
        }
        let mut child = cmd
            .env("LIVI_NO_UI", "1")
            .env(LIFELINE_ENV, "1")
            .env("LIVI_UI_PLANES", planes)
            .stdin(Stdio::piped())
            .spawn()?;
        let pid = child.id() as i32;
        if let Some(pipe) = child.stdin.take()
            && let Err(e) = crate::log::to_core(pipe)
        {
            eprintln!("[ui] the log stays on the terminal: {e}");
        }
        let (tx, ended) = mpsc::channel();
        thread::Builder::new()
            .name("core-wait".into())
            .spawn(move || {
                let code = child.wait().ok().and_then(|s| s.code());
                println!("[ui] core ended ({code:?})");
                let _ = tx.send(code);
                on_exit(code);
            })?;
        Ok(Self { pid, ended })
    }

    pub fn stop(self, wait: Duration) -> Option<i32> {
        if let Ok(code) = self.ended.try_recv() {
            return code;
        }
        unsafe { libc::kill(self.pid, libc::SIGTERM) };
        self.ended.recv_timeout(wait).ok().flatten()
    }
}
