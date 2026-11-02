// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use crate::{control, screens};

pub(crate) fn bind(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let _ = std::fs::remove_file(path);
    UnixListener::bind(path)
}

pub(crate) fn accept(
    name: &str,
    listener: UnixListener,
    serve: fn(UnixStream),
) -> std::io::Result<()> {
    let name = name.to_string();
    std::thread::Builder::new()
        .name(name.clone())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let _ = std::thread::Builder::new()
                    .name(name.clone())
                    .spawn(move || serve(conn));
            }
        })?;
    Ok(())
}

pub fn serve(control: &Path) -> std::io::Result<()> {
    accept("video-control", bind(control)?, serve_control)
}

fn serve_control(conn: UnixStream) {
    let Ok(mut answers) = conn.try_clone() else {
        return;
    };
    for line in BufReader::new(conn).lines() {
        let Ok(line) = line else { return };
        let Some(answer) = control::parse(&line).and_then(screens::apply) else {
            continue;
        };
        if answers.write_all(answer.as_bytes()).is_err() {
            return;
        }
    }
}
