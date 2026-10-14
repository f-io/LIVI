use std::f64::consts::PI;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use livi_core_proto::state::State;
use livi_media::gst_host::{AudioCodec, AudioOpts, GstHost};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::hub::Hub;

const RATE: u32 = 48_000;
const CHANNELS: u8 = 2;
/// The renderer's blink phase uses the same period, so sound and lamp agree.
const BLINK_PERIOD_MS: u64 = 500;
const GENERATE_EVERY: Duration = Duration::from_millis(30);
/// The stream stays warm a moment, a blinker that comes right back reuses it.
const TEARDOWN_GRACE: Duration = Duration::from_millis(600);
const MAX_CATCHUP_FRAMES: u64 = (RATE / 4) as u64;
const DEFAULT_VOLUME: f64 = 0.7;

/// The level curve the projection's streams use, -60 dB to 0 dB.
fn gain(volume: f64) -> f64 {
    let v = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 0.0 };
    if v <= 0.0 { 0.0 } else { 10f64.powf((-60.0 + 60.0 * v) / 20.0) }
}

fn relay_click(on: bool) -> Vec<f32> {
    let rate = f64::from(RATE);
    let len = (rate * if on { 34.0 } else { 42.0 } / 1000.0) as usize;
    let mut out = vec![0f32; len];
    let mut seed: u64 = if on { 0x9e37_79b1 } else { 0x85eb_ca77 };
    let mut noise = || {
        seed = (seed * 1_664_525 + 1_013_904_223) & 0xffff_ffff;
        seed as f64 / f64::from(u32::MAX) * 2.0 - 1.0
    };
    let modes: [(f64, f64, f64); 3] = if on {
        [(2100.0, 0.009, 1.0), (3450.0, 0.006, 0.5), (5200.0, 0.0038, 0.28)]
    } else {
        [(1300.0, 0.012, 1.0), (2050.0, 0.008, 0.46), (3050.0, 0.005, 0.24)]
    };
    let (mut hp_in, mut hp_out) = (0.0, 0.0);
    let hp_coeff = 0.92;
    for (i, sample) in out.iter_mut().enumerate() {
        let t = i as f64 / rate;
        let mut s: f64 = modes
            .iter()
            .map(|(f, decay, amp)| amp * (2.0 * PI * f * t).sin() * (-t / decay).exp())
            .sum();
        s *= 0.5;
        s += 0.22 * (2.0 * PI * 175.0 * t).sin() * (-t / 0.004).exp();
        let nz = noise() * (-t / 0.0016).exp();
        let hp = hp_coeff * (hp_out + nz - hp_in);
        hp_in = nz;
        hp_out = hp;
        s += 0.5 * hp;
        *sample = s as f32;
    }
    let attack = ((rate * 0.0004) as usize).max(1);
    for (i, sample) in out.iter_mut().take(attack).enumerate() {
        *sample = (f64::from(*sample) * (i as f64 / attack as f64)) as f32;
    }
    let lp = if on { 0.5 } else { 0.38 };
    let mut y = 0.0;
    for sample in &mut out {
        y += lp * (f64::from(*sample) - y);
        *sample = y as f32;
    }
    let peak = out.iter().fold(0f64, |p, s| p.max(f64::from(s.abs())));
    let norm = 0.85 / peak;
    for sample in &mut out {
        *sample = (f64::from(*sample) * norm) as f32;
    }
    out
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

struct Clicks {
    on: Vec<f32>,
    off: Vec<f32>,
    start_ms: f64,
    produced: u64,
    last_blink: u64,
    playing: Option<(bool, usize)>,
}

impl Clicks {
    fn new(start_ms: f64) -> Self {
        Self {
            on: relay_click(true),
            off: relay_click(false),
            start_ms,
            produced: 0,
            last_blink: (start_ms as u64) / BLINK_PERIOD_MS,
            playing: None,
        }
    }

    /// s16le stereo.
    fn generate(&mut self, now_ms: f64, active: bool, gain: f64) -> Vec<u8> {
        let rate = f64::from(RATE);
        let target = (((now_ms - self.start_ms) / 1000.0) * rate).max(0.0) as u64;
        if target <= self.produced {
            return Vec::new();
        }
        let mut n = target - self.produced;
        if n > MAX_CATCHUP_FRAMES {
            self.produced = target - MAX_CATCHUP_FRAMES;
            n = MAX_CATCHUP_FRAMES;
        }
        let mut pcm = Vec::with_capacity(n as usize * 4);
        for i in 0..n {
            let frame = self.produced + i;
            let t_ms = self.start_ms + frame as f64 / rate * 1000.0;
            let blink = (t_ms as u64) / BLINK_PERIOD_MS;
            if blink != self.last_blink {
                self.last_blink = blink;
                if active {
                    self.playing = Some((blink.is_multiple_of(2), 0));
                }
            }
            let mut s = 0.0;
            if let Some((on, pos)) = self.playing.as_mut() {
                let wave = if *on { &self.on } else { &self.off };
                s = f64::from(wave[*pos]);
                *pos += 1;
                if *pos >= wave.len() {
                    self.playing = None;
                }
            }
            let v = (s * gain * 32767.0).clamp(-32768.0, 32767.0) as i16;
            pcm.extend_from_slice(&v.to_le_bytes());
            pcm.extend_from_slice(&v.to_le_bytes());
        }
        self.produced += n;
        pcm
    }
}

fn blinking(state: &State) -> bool {
    let t = &state.telemetry;
    matches!(t.get("turnSignal").and_then(|v| v.as_str()), Some("left" | "right"))
        || t.get("hazards").and_then(serde_json::Value::as_bool) == Some(true)
}

struct Stream {
    id: u32,
    device: String,
    clicks: Clicks,
}

async fn open(gst: &GstHost, device: &str) -> Option<Stream> {
    let opts = AudioOpts {
        codec: AudioCodec::PcmLe,
        payload_type: 0,
        clock_rate: RATE,
        channels: CHANNELS,
        latency_ms: 0,
        realtime: true,
        fed: true,
        device: device.to_string(),
    };
    let (id, _, _) = gst.open_audio(&[0; 32], &opts).await;
    if id == 0 {
        return None;
    }
    gst.set_audio_active(id, true);
    Some(Stream { id, device: device.to_string(), clicks: Clicks::new(now_ms()) })
}

pub async fn run(hub: Arc<Hub>, gst: GstHost) {
    let mut state: watch::Receiver<State> = hub.watch();
    let mut stream: Option<Stream> = None;
    let mut active = false;
    let mut close_at: Option<Instant> = None;
    let mut tick = tokio::time::interval(GENERATE_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let (wants, muted, device, volume) = {
            let s = state.borrow_and_update();
            let cfg = &s.config;
            (
                blinking(&s),
                cfg.disable_audio_output,
                cfg.audio_output_device.clone().unwrap_or_default(),
                cfg.system_sounds_volume.unwrap_or(DEFAULT_VOLUME),
            )
        };
        if muted || stream.as_ref().is_some_and(|s| s.device != device) {
            if let Some(s) = stream.take() {
                gst.close_audio(s.id);
            }
            close_at = None;
        }
        if wants && !muted {
            close_at = None;
            if stream.is_none() {
                stream = open(&gst, &device).await;
            }
        } else if active && stream.is_some() {
            close_at = Some(Instant::now() + TEARDOWN_GRACE);
        }
        active = wants && !muted;
        let closing = close_at;
        tokio::select! {
            changed = state.changed() => if changed.is_err() { return },
            _ = tick.tick(), if stream.is_some() => {
                if let Some(s) = stream.as_mut() {
                    let pcm = s.clicks.generate(now_ms(), active, gain(volume));
                    if !pcm.is_empty() {
                        gst.push_audio(s.id, &pcm);
                    }
                }
            }
            () = async {
                match closing {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                close_at = None;
                if let Some(s) = stream.take() {
                    gst.close_audio(s.id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The reference click: length, energy and every 200th sample.
    const ON: (usize, f64, &[f64]) = (
        1632,
        30.34130957554999,
        &[
            0.0,
            -0.30132102966308594,
            0.018408259376883507,
            0.13530194759368896,
            -0.016851350665092468,
            -0.053182441741228104,
            0.011089103296399117,
            0.017764342948794365,
            -0.003385694930329919,
        ],
    );
    const OFF: (usize, f64, &[f64]) = (
        2016,
        46.60901726614324,
        &[
            0.0,
            0.1431623101234436,
            -0.22356781363487244,
            0.17759710550308228,
            -0.08320481330156326,
            0.010142878629267216,
            0.0279140193015337,
            -0.03974231332540512,
            0.035841021686792374,
            -0.02478453889489174,
            0.012813232839107513,
        ],
    );

    #[test]
    fn the_click_sounds_as_before() {
        for (on, (len, energy, every)) in [(true, ON), (false, OFF)] {
            let click = relay_click(on);
            assert_eq!(click.len(), len);
            let e: f64 = click.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
            assert!((e - energy).abs() < 1e-4, "energy {e} vs {energy}");
            let peak = click.iter().fold(0f32, |p, s| p.max(s.abs()));
            assert_eq!(peak, 0.85);
            for (i, want) in every.iter().enumerate() {
                assert!((f64::from(click[i * 200]) - want).abs() < 1e-6, "sample {}", i * 200);
            }
        }
        assert_eq!(gain(0.0), 0.0);
        assert_eq!(gain(1.0), 1.0);
        assert!((gain(0.7) - 0.125_892_541_179_416_73).abs() < 1e-12);
        assert_eq!(gain(f64::NAN), 0.0);
    }

    #[test]
    fn a_click_lands_on_each_blink_edge_while_active() {
        let mut clicks = Clicks::new(1000.0);
        assert!(clicks.generate(1000.0, true, 1.0).is_empty());
        for until in [1250.0, 1500.0] {
            let pcm = clicks.generate(until, true, 1.0);
            assert_eq!(pcm.len(), 12_000 * 4);
            assert!(pcm.iter().all(|b| *b == 0));
        }
        let pcm = clicks.generate(1750.0, true, 1.0);
        assert!(pcm[..4].iter().all(|b| *b == 0) && pcm.iter().any(|b| *b != 0));
        let quiet = clicks.generate(10_000.0, false, 1.0);
        assert_eq!(quiet.len(), MAX_CATCHUP_FRAMES as usize * 4);
        assert!(quiet.iter().all(|b| *b == 0));
    }

    #[test]
    fn a_blinker_or_the_hazards_make_it_tick() {
        let livi = livi_core_proto::state::PerScreen {
            main: livi_core_proto::state::Front::Livi,
            cluster: livi_core_proto::state::Front::Livi,
            aux: livi_core_proto::state::Front::Livi,
        };
        let mut s = State {
            front: livi,
            sessions: Default::default(),
            now_playing: Default::default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            telemetry: Default::default(),
            update: Default::default(),
            config: crate::config_file::defaults(),
        };
        assert!(!blinking(&s));
        s.telemetry.insert("turnSignal".into(), json!("left"));
        assert!(blinking(&s));
        s.telemetry.insert("turnSignal".into(), json!("none"));
        s.telemetry.insert("hazards".into(), json!(true));
        assert!(blinking(&s));
    }
}
