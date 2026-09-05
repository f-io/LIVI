//! The CarPlay microphone stream sent from this process: livi-audio-player captures and
//! encodes it, livi-audio-uplink seals the packets.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use livi_audio_player::uplink::{Uplink, UplinkConfig};

static UPLINKS: OnceLock<Mutex<HashMap<u32, Uplink>>> = OnceLock::new();
static NEXT_ID: AtomicU32 = AtomicU32::new(0x7d00_0001);

fn uplinks() -> &'static Mutex<HashMap<u32, Uplink>> {
    UPLINKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Starts capturing and sending. Returns the id the caller closes it by.
pub fn open(cfg: UplinkConfig) -> Option<u32> {
    let uplink = Uplink::new(cfg)?;
    uplink.start();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    uplinks().lock().unwrap_or_else(|e| e.into_inner()).insert(id, uplink);
    eprintln!("[cp_mic] uplink 0x{id:x} open");
    Some(id)
}

pub fn close(id: u32) {
    if uplinks().lock().unwrap_or_else(|e| e.into_inner()).remove(&id).is_some() {
        eprintln!("[cp_mic] uplink 0x{id:x} close");
    }
}
