// A fresh unix socket the main process attaches to, one per session. The
// transport (TCP for wireless AA, USB bulk for wired AA and the dongle) names
// its own stem, the counter keeps the paths unique.

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::UnixListener;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// A fresh socket for the main process to attach to one session on.
pub fn bind_session_socket(stem: &str) -> Option<(UnixListener, String)> {
    let n = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let path = format!("/tmp/{stem}-{n}.sock");
    let _ = std::fs::remove_file(&path);
    let node = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[session] session socket {path}: {e}");
            return None;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666));
    Some((node, path))
}
