//! The process around the host: the socket the main process serves, the GLib
//! main loop the pipelines need, and the backtrace a crash leaves behind.

use core::ffi::{c_char, c_int, c_void};
use std::cell::RefCell;
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

use glib::IOCondition;

use crate::gst::Gst;
use crate::{Host, Wire};

/// How often every receiver reports what it saw.
const STATS_SECONDS: u32 = 5;
/// How often the visualizer samples are drained to the main process.
const VISUALIZER_INTERVAL_MS: u64 = 20;
const CHUNK: usize = 65536;

/// The audio receive threads reply through the same socket, so writes take the
/// lock to keep the framed messages from interleaving.
struct SocketWire {
    sock: Arc<UnixStream>,
    write: Mutex<()>,
}

impl Wire for SocketWire {
    fn reply(&self, op: u8, id: u32, rest: &[u8]) {
        let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let mut sock: &UnixStream = &self.sock;
        let _ = sock.write_all(&livi_host_proto::encode_reply(op, id, rest));
    }
}

/// Connects to the host socket and runs the GLib main loop. In a process of
/// its own, libwayland binds the system libffi, which keeps wayland marshalling
/// intact across resizes.
pub fn run(sock_path: &str, crash_log: &str) {
    glib::set_prgname(Some("livi-video"));
    livi_video_player::ensure_init();
    arm_crash_handler(crash_log);

    let Ok(sock) = UnixStream::connect(sock_path) else {
        eprintln!("[gst-host] connect to {sock_path} failed");
        std::process::exit(1);
    };
    let sock = Arc::new(sock);
    let wire = Arc::new(SocketWire { sock: sock.clone(), write: Mutex::new(()) });
    let host = Rc::new(RefCell::new(Host::new(Gst, wire)));

    let fd = sock.as_raw_fd();
    let reading = host.clone();
    glib::unix_fd_add_local(
        fd,
        IOCondition::IN | IOCondition::HUP | IOCondition::ERR,
        move |_, cond| {
            if cond.contains(IOCondition::HUP) || cond.contains(IOCondition::ERR) {
                std::process::exit(0);
            }
            let mut chunk = [0u8; CHUNK];
            let mut from: &UnixStream = &sock;
            let read = match from.read(&mut chunk) {
                Ok(0) | Err(_) => std::process::exit(0),
                Ok(n) => n,
            };
            reading.borrow_mut().feed(&chunk[..read]);
            glib::ControlFlow::Continue
        },
    );

    let visualizer_host = host.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(VISUALIZER_INTERVAL_MS), move || {
        visualizer_host.borrow().pump_visualizer();
        glib::ControlFlow::Continue
    });

    glib::timeout_add_seconds_local(STATS_SECONDS, move || {
        for line in host.borrow().take_stats() {
            eprintln!("{line}");
        }
        glib::ControlFlow::Continue
    });

    glib::MainLoop::new(None, false).run();
}

/// Where the backtrace goes besides stderr. Set before the handler arms and
/// read from the signal handler, so it stays a plain pointer.
static CRASH_PATH: AtomicPtr<c_char> = AtomicPtr::new(core::ptr::null_mut());

unsafe extern "C" {
    fn backtrace(buffer: *mut *mut c_void, size: c_int) -> c_int;
    fn backtrace_symbols_fd(buffer: *const *mut c_void, size: c_int, fd: c_int);
}

extern "C" fn on_crash(sig: c_int) {
    const HEADER: &[u8] = b"\n=== gst-host CRASH backtrace ===\n";
    let mut frames = [core::ptr::null_mut::<c_void>(); 64];
    let depth = unsafe { backtrace(frames.as_mut_ptr(), frames.len() as c_int) };

    let dump = |fd: c_int| unsafe {
        let _ = libc::write(fd, HEADER.as_ptr().cast(), HEADER.len());
        backtrace_symbols_fd(frames.as_ptr(), depth, fd);
    };
    dump(libc::STDERR_FILENO);

    let path = CRASH_PATH.load(Ordering::Relaxed);
    if !path.is_null() {
        let fd = unsafe {
            libc::open(path, libc::O_CREAT | libc::O_WRONLY | libc::O_TRUNC, 0o644 as c_int)
        };
        if fd >= 0 {
            dump(fd);
            unsafe { libc::close(fd) };
        }
    }

    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

fn arm_crash_handler(crash_log: &str) {
    if !crash_log.is_empty()
        && let Ok(path) = CString::new(crash_log)
    {
        CRASH_PATH.store(path.into_raw(), Ordering::Relaxed);
    }
    unsafe {
        libc::signal(libc::SIGSEGV, on_crash as *const () as libc::sighandler_t);
        libc::signal(libc::SIGABRT, on_crash as *const () as libc::sighandler_t);
    }
}
