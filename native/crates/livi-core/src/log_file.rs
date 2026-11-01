// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const KEEP: usize = 5;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const DRAIN: Duration = Duration::from_millis(500);

static TERMINAL: OnceLock<(RawFd, RawFd)> = OnceLock::new();
static SINK: OnceLock<Arc<Mutex<Sink>>> = OnceLock::new();
static READERS: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
fn name(i: usize) -> String {
    if i == 0 {
        "LIVI.log".into()
    } else {
        format!("LIVI.{i}.log")
    }
}

fn rotate(dir: &Path) {
    let _ = fs::remove_file(dir.join(name(KEEP - 1)));
    for i in (0..KEEP - 1).rev() {
        let _ = fs::rename(dir.join(name(i)), dir.join(name(i + 1)));
    }
}

struct Sink {
    dir: PathBuf,
    file: File,
    written: u64,
}

impl Sink {
    fn open(dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        rotate(dir);
        let file = File::create(dir.join(name(0)))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            file,
            written: 0,
        })
    }

    fn line(&mut self, line: &str) {
        if self.written > MAX_BYTES
            && let Ok(next) = Self::open(&self.dir)
        {
            *self = next;
        }
        if self.file.write_all(format!("{line}\n").as_bytes()).is_ok() {
            self.written += line.len() as u64 + 1;
        }
    }
}

fn stamp(now: &jiff::Zoned) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond()
    )
}

pub fn start(dir: &Path) {
    if TERMINAL.get().is_some() {
        return;
    }
    let sink = match Sink::open(dir) {
        Ok(sink) => Arc::new(Mutex::new(sink)),
        Err(e) => {
            eprintln!("[core] no log file in {}: {e}", dir.display());
            return;
        }
    };
    let _ = std::io::stdout().flush();
    let (Some(out), Some(err)) = (follow(1, sink.clone()), follow(2, sink.clone())) else {
        return;
    };
    let _ = TERMINAL.set((out, err));
    let _ = SINK.set(sink);
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report(info);
        if std::thread::current().name() == Some("main") {
            finish();
        }
    }));
}

pub fn follow_ui(gone: impl FnOnce() + Send + 'static) {
    let sink = SINK.get().cloned();
    let _ = std::thread::Builder::new()
        .name("log-ui".into())
        .spawn(move || {
            copy_lines(
                std::io::stdin().lock(),
                &mut std::io::sink(),
                sink.as_deref(),
                false,
            );
            gone();
        });
}

pub fn finish() {
    let _ = std::io::stdout().flush();
    let Some(&(out, err)) = TERMINAL.get() else {
        return;
    };
    // SAFETY: both are descriptors this process opened and never closes.
    unsafe {
        libc::dup2(out, 1);
        libc::dup2(err, 2);
    }
    let readers = std::mem::take(&mut *READERS.lock().unwrap_or_else(|e| e.into_inner()));
    let until = Instant::now() + DRAIN;
    while readers.iter().any(|r| !r.is_finished()) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn follow(fd: RawFd, sink: Arc<Mutex<Sink>>) -> Option<RawFd> {
    let mut ends = [0; 2];
    // SAFETY: plain descriptor calls on descriptors this process owns.
    let terminal = unsafe {
        if libc::pipe(ends.as_mut_ptr()) != 0 {
            return None;
        }
        libc::fcntl(ends[0], libc::F_SETFD, libc::FD_CLOEXEC);
        let terminal = libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3);
        if terminal < 0 || libc::dup2(ends[1], fd) < 0 {
            libc::close(ends[0]);
            libc::close(ends[1]);
            if terminal >= 0 {
                libc::close(terminal);
            }
            return None;
        }
        libc::close(ends[1]);
        terminal
    };
    // SAFETY: the read end is ours alone from here on.
    let reader = unsafe { File::from_raw_fd(ends[0]) };
    let spawned = std::thread::Builder::new()
        .name(format!("log-fd{fd}"))
        .spawn(move || {
            // SAFETY: finish() still needs it, so it is never closed.
            let mut screen = ManuallyDrop::new(unsafe { File::from_raw_fd(terminal) });
            copy_lines(BufReader::new(reader), &mut *screen, Some(&sink), true);
        });
    let reader = spawned.ok()?;
    READERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(reader);
    Some(terminal)
}

fn copy_lines(
    mut from: impl BufRead,
    screen: &mut impl Write,
    sink: Option<&Mutex<Sink>>,
    stamp_it: bool,
) {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match from.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buf);
        let text = text.trim_end_matches('\n');
        let line = if stamp_it {
            format!("[{}] {text}", stamp(&jiff::Zoned::now()))
        } else {
            text.to_string()
        };
        let _ = screen.write_all(format!("{line}\n").as_bytes());
        if let Some(sink) = sink {
            sink.lock().unwrap_or_else(|e| e.into_inner()).line(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;

    fn read(dir: &Path, i: usize) -> String {
        fs::read_to_string(dir.join(name(i))).unwrap_or_default()
    }

    #[test]
    fn each_start_moves_the_older_logs_up_and_drops_the_oldest() {
        let dir = TempDir::new();
        for i in 0..KEEP {
            fs::write(dir.0.join(name(i)), format!("run {i}")).unwrap();
        }
        let mut sink = Sink::open(&dir.0).unwrap();
        sink.line("fresh");
        assert_eq!(read(&dir.0, 0), "fresh\n");
        assert_eq!(read(&dir.0, 1), "run 0");
        assert_eq!(read(&dir.0, 4), "run 3");
        assert!(!dir.0.join(name(KEEP)).exists());
    }

    #[test]
    fn a_full_file_gives_way_to_a_new_one() {
        let dir = TempDir::new();
        let mut sink = Sink::open(&dir.0).unwrap();
        sink.line("old");
        sink.written = MAX_BYTES + 1;
        sink.line("new");
        assert_eq!(read(&dir.0, 0), "new\n");
        assert_eq!(read(&dir.0, 1), "old\n");
    }

    #[test]
    fn every_line_gets_the_time_on_screen_and_in_the_file() {
        let dir = TempDir::new();
        let sink = Mutex::new(Sink::open(&dir.0).unwrap());
        let mut screen = Vec::new();
        copy_lines(
            &b"[core] up\nnot \xff utf-8\nno newline"[..],
            &mut screen,
            Some(&sink),
            true,
        );
        let screen = String::from_utf8(screen).unwrap();
        let lines: Vec<&str> = screen.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with("] [core] up"));
        assert!(lines[1].ends_with("] not \u{fffd} utf-8"));
        assert!(lines[2].ends_with("] no newline"));
        assert_eq!(read(&dir.0, 0), screen);
        let stamped = &lines[0][..14];
        assert!(
            stamped.starts_with('[') && stamped.ends_with(']'),
            "{stamped}"
        );
    }

    #[test]
    fn the_uis_lines_keep_their_own_time() {
        let dir = TempDir::new();
        let sink = Mutex::new(Sink::open(&dir.0).unwrap());
        let mut screen = Vec::new();
        copy_lines(
            &b"[07:08:09.012] [window] up\n"[..],
            &mut screen,
            Some(&sink),
            false,
        );
        assert_eq!(read(&dir.0, 0), "[07:08:09.012] [window] up\n");
    }

    #[test]
    fn the_time_has_milliseconds() {
        let at: jiff::Zoned = "2026-10-05T07:08:09.012+02:00[+02:00]".parse().unwrap();
        assert_eq!(stamp(&at), "07:08:09.012");
    }
}
