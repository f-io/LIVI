//! The helper's media feed on platforms where the pipelines run inside the main
//! process: the socket, the keyframe gate per stream, and the registries the
//! records are routed by. On Linux the host process does the same.

use std::collections::HashMap;
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use livi_audio_player::Player as AudioPlayer;
use livi_host_proto::feed::{self as feedproto, Framer};
use livi_host_proto::{CLUSTER_PLANE_MAX, CLUSTER_PLANE_MIN, CLUSTER_RECV_ID, feeder_of};
use livi_video_fanout::Fanout;
use livi_video_nal::CpCodec;
use livi_video_player::Player;

/// One audio stream, fed by the main process or by the helper.
pub struct AudioOut {
    pub id: u32,
    pub player: AudioPlayer,
    /// A held session keeps its stream, but its samples stop here.
    pub active: AtomicBool,
}

#[derive(Default)]
struct Registry {
    planes: HashMap<u32, Arc<Player>>,
    audio: HashMap<u32, Arc<AudioOut>>,
    /// The keyframe gate per fed video stream, None for a codec it cannot read.
    fans: HashMap<u32, Option<Fanout>>,
    next_audio_id: u32,
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
/// Bumped per accepted connection, the reader of an older one stops.
static GENERATION: AtomicU64 = AtomicU64::new(0);
const FIRST_AUDIO_ID: u32 = 0x7c00_0001;

fn lock() -> MutexGuard<'static, Registry> {
    REGISTRY
        .get_or_init(|| Mutex::new(Registry { next_audio_id: FIRST_AUDIO_ID, ..Default::default() }))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Puts a started plane under its id and replays what its stream has cached, so
/// a plane created mid-stream shows a picture before the next keyframe.
pub fn register_plane(id: u32, player: Arc<Player>) {
    let mut reg = lock();
    if let Some(Some(fan)) = reg.fans.get(&feeder_of(id)) {
        for frame in fan.cached() {
            player.push(frame);
        }
    }
    reg.planes.insert(id, player);
}

pub fn unregister_plane(id: u32) {
    lock().planes.remove(&id);
}

pub fn register_audio(player: AudioPlayer) -> Arc<AudioOut> {
    let mut reg = lock();
    let id = reg.next_audio_id;
    reg.next_audio_id += 1;
    let out = Arc::new(AudioOut { id, player, active: AtomicBool::new(false) });
    reg.audio.insert(id, out.clone());
    out
}

pub fn unregister_audio(id: u32) {
    lock().audio.remove(&id);
}

/// Binds the feed socket and serves it from threads of its own.
pub fn open(path: &str) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    std::thread::Builder::new().name("livi-feed".into()).spawn(move || {
        for conn in listener.incoming() {
            let Ok(sock) = conn else { continue };
            // a new helper connection replaces the one before it
            let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            eprintln!("[feed] connection accepted");
            let _ = std::thread::Builder::new()
                .name("livi-feed-read".into())
                .spawn(move || read_loop(sock, generation));
        }
    })?;
    Ok(())
}

fn read_loop(mut sock: UnixStream, generation: u64) {
    let mut framer = Framer::new();
    let mut chunk = vec![0u8; 65536];
    loop {
        let n = match sock.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if GENERATION.load(Ordering::SeqCst) != generation {
            break;
        }
        framer.push(&chunk[..n]);
        while let Some(record) = framer.next_record() {
            dispatch(record);
        }
    }
    if GENERATION.load(Ordering::SeqCst) == generation {
        eprintln!("[feed] connection closed");
    }
}

fn dispatch(r: feedproto::Record) {
    match r.kind {
        feedproto::KIND_VIDEO_START => {
            let fan = match r.payload.first() {
                Some(0) => Some(CpCodec::H264),
                Some(1) => Some(CpCodec::H265),
                _ => None,
            }
            .map(|codec| {
                let mut fan = Fanout::new();
                fan.set_codec(codec);
                fan.set_active(true);
                fan
            });
            lock().fans.insert(r.id, fan);
        }
        feedproto::KIND_VIDEO => {
            // The pushes happen outside the lock, a full source may block.
            let targets = {
                let mut reg = lock();
                let targets = targets_of(&reg.planes, r.id);
                let pass = match reg.fans.get_mut(&r.id) {
                    Some(Some(fan)) => fan.take(&r.payload, !targets.is_empty()),
                    _ => !targets.is_empty(),
                };
                if pass { targets } else { Vec::new() }
            };
            for plane in targets {
                plane.push(&r.payload);
            }
        }
        feedproto::KIND_AUDIO => {
            let out = lock().audio.get(&r.id).cloned();
            if let Some(out) = out
                && out.active.load(Ordering::Relaxed)
            {
                out.player.push_samples(&r.payload);
            }
        }
        _ => {}
    }
}

/// The cluster id serves every cluster plane, any other id its own plane.
fn targets_of(planes: &HashMap<u32, Arc<Player>>, id: u32) -> Vec<Arc<Player>> {
    if id == CLUSTER_RECV_ID {
        (CLUSTER_PLANE_MIN..=CLUSTER_PLANE_MAX).filter_map(|cid| planes.get(&cid).cloned()).collect()
    } else {
        planes.get(&id).cloned().into_iter().collect()
    }
}
