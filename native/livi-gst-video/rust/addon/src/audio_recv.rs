//! A CarPlay audio stream received in this process: livi-audio-stream receives, frames and
//! decrypts it, livi-audio-player plays it, the feed registry addresses it.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

use livi_audio_player::{Config as AudioConfig, Player as AudioPlayer};
use livi_audio_stream::receiver::AudioReceiver;
use livi_audio_stream::{AudioSink, AudioStream};

use crate::feed::{self, AudioOut};

/// Reports the stream's first sample, which the caller answers the phone's SETUP with.
pub type StartedCb = Box<dyn Fn(u32, u32) + Send + 'static>;

struct Sink {
    out: Arc<AudioOut>,
    on_started: StartedCb,
}

impl AudioSink for Sink {
    fn on_started(&mut self, first_sample: u32) {
        (self.on_started)(self.out.id, first_sample);
    }

    fn on_rtp(&mut self, rtp: &[u8], _sample: u32) {
        if self.out.active.load(Ordering::Relaxed) {
            self.out.player.push_rtp(rtp);
        }
    }
}

/// Only the ports belong here; everything about the stream itself lives in the feed registry.
static EARS: OnceLock<Mutex<HashMap<u32, AudioReceiver>>> = OnceLock::new();

fn ears() -> &'static Mutex<HashMap<u32, AudioReceiver>> {
    EARS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Builds the pipeline for this stream and binds its two ports. Returns the id the caller
/// addresses it by, plus the data and control port for the SETUP reply.
pub fn open(cfg: &AudioConfig, key: [u8; 32], on_started: StartedCb) -> Option<(u32, u16, u16)> {
    let player = AudioPlayer::new(cfg)?;
    player.start();
    let out = feed::register_audio(player);

    let stream = AudioStream::new(key, Box::new(Sink { out: out.clone(), on_started }));
    let (recv, data_port, control_port) = match AudioReceiver::new(stream) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[cp_audio] cannot listen: {e}");
            out.player.stop();
            feed::unregister_audio(out.id);
            return None;
        }
    };
    ears().lock().unwrap_or_else(|e| e.into_inner()).insert(out.id, recv);
    eprintln!("[cp_audio] stream 0x{:x} open ({:?})", out.id, cfg.codec);
    Some((out.id, data_port, control_port))
}

pub fn set_active(id: u32, on: bool) {
    if let Some(out) = feed::audio_out(id) {
        out.active.store(on, Ordering::Relaxed);
    }
}

pub fn set_volume(id: u32, level: f64, ramp_ms: u32) {
    if let Some(out) = feed::audio_out(id) {
        out.player.set_volume(level, u64::from(ramp_ms));
    }
}

pub fn close(id: u32) {
    eprintln!("[cp_audio] stream 0x{id:x} close");
    ears().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    if let Some(out) = feed::audio_out(id) {
        out.player.stop();
    }
    feed::unregister_audio(id);
}
