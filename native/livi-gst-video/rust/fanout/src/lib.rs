//! Which frames a receiver lets through, and the GOP it keeps.
//!
//! Frames pass from the first keyframe on. The running GOP is kept up to
//! `CACHE_MAX` frames and primes a player created mid-stream.

use livi_video_nal::{classify_nal, CpCodec, CpNalKind};

/// Frames kept at most. A longer GOP is dropped.
pub const CACHE_MAX: usize = 240;

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stats {
    pub incoming: u64,
    pub dropped: u64,
    pub pushed: u64,
}

pub struct Fanout {
    active: bool,
    have_codec: bool,
    codec: CpCodec,
    awaiting_keyframe: bool,
    cache: Vec<Vec<u8>>,
    cache_valid: bool,
    stats: Stats,
}

impl Default for Fanout {
    fn default() -> Self {
        Self::new()
    }
}

impl Fanout {
    pub fn new() -> Self {
        Self {
            active: false,
            have_codec: false,
            codec: CpCodec::H264,
            awaiting_keyframe: true,
            cache: Vec::new(),
            cache_valid: false,
            stats: Stats::default(),
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_codec(&mut self, codec: CpCodec) {
        self.codec = codec;
        self.have_codec = true;
    }

    /// Waits for the next keyframe again and drops the cached GOP.
    pub fn restart(&mut self) {
        self.awaiting_keyframe = true;
        self.cache.clear();
        self.cache_valid = false;
    }

    pub fn awaiting_keyframe(&self) -> bool {
        self.awaiting_keyframe
    }

    /// The frames a new player is primed with.
    pub fn cached(&self) -> &[Vec<u8>] {
        if self.cache_valid { &self.cache } else { &[] }
    }

    /// Reads the counters and starts them over.
    pub fn take_stats(&mut self) -> Stats {
        core::mem::take(&mut self.stats)
    }

    /// Accounts for `nal` and answers whether the caller pushes it. `has_target`
    /// says whether any player is listening. The frame enters the cache whether
    /// or not it is pushed.
    pub fn take(&mut self, nal: &[u8], has_target: bool) -> bool {
        self.stats.incoming += 1;
        if !self.have_codec {
            self.stats.dropped += 1;
            return false;
        }

        let kind = classify_nal(nal, self.codec);
        if self.awaiting_keyframe {
            self.stats.dropped += 1;
            match kind {
                // parameter sets ride along, the keyframe needs them
                CpNalKind::Delta => return false,
                CpNalKind::Keyframe => self.awaiting_keyframe = false,
                CpNalKind::Params => {}
            }
        }

        if kind == CpNalKind::Keyframe {
            self.cache.clear();
            self.cache_valid = true;
        }
        if self.cache_valid {
            if self.cache.len() >= CACHE_MAX {
                self.cache.clear();
                self.cache_valid = false;
            } else {
                self.cache.push(nal.to_vec());
            }
        }

        // A held receiver fills its cache but pushes nothing.
        if !self.active {
            return false;
        }
        if has_target {
            self.stats.pushed += 1;
        }
        has_target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nal(kind: u8, body_len: usize) -> Vec<u8> {
        let mut v = ((body_len + 1) as u32).to_be_bytes().to_vec();
        v.push(kind);
        v.extend(core::iter::repeat_n(0u8, body_len));
        v
    }

    fn keyframe() -> Vec<u8> {
        nal(5, 3)
    }
    fn delta() -> Vec<u8> {
        nal(1, 3)
    }
    fn params() -> Vec<u8> {
        nal(7, 3)
    }

    fn running() -> Fanout {
        let mut f = Fanout::new();
        f.set_active(true);
        f.set_codec(CpCodec::H264);
        f
    }

    #[test]
    fn nothing_passes_before_the_first_keyframe() {
        let mut f = running();

        assert!(!f.take(&delta(), true));
        assert!(!f.take(&delta(), true));
        assert!(f.take(&keyframe(), true));
        assert!(f.take(&delta(), true));
    }

    #[test]
    fn a_frame_without_a_known_codec_is_dropped() {
        let mut f = Fanout::new();
        f.set_active(true);

        assert!(!f.take(&keyframe(), true));
        assert_eq!(f.take_stats(), Stats { incoming: 1, dropped: 1, pushed: 0 });
    }

    #[test]
    fn an_inactive_receiver_fills_its_cache_and_pushes_nothing() {
        let mut f = Fanout::new();
        f.set_codec(CpCodec::H264);

        assert!(!f.take(&keyframe(), true));
        assert_eq!(f.take_stats(), Stats { incoming: 1, dropped: 1, pushed: 0 });
        assert_eq!(f.cached().len(), 1);
    }

    #[test]
    fn a_receiver_made_active_has_the_running_gop_ready() {
        let mut f = Fanout::new();
        f.set_codec(CpCodec::H264);
        f.take(&keyframe(), true);
        f.take(&delta(), true);

        f.set_active(true);

        assert_eq!(f.cached().len(), 2);
        assert!(!f.awaiting_keyframe());
    }

    #[test]
    fn parameter_sets_pass_while_the_keyframe_is_still_awaited() {
        let mut f = running();

        assert!(f.take(&params(), true));
        assert!(f.awaiting_keyframe());
        assert!(!f.take(&delta(), true));
    }

    #[test]
    fn the_cache_starts_over_at_every_keyframe() {
        let mut f = running();
        f.take(&keyframe(), true);
        f.take(&delta(), true);
        f.take(&delta(), true);
        assert_eq!(f.cached().len(), 3);

        f.take(&keyframe(), true);

        assert_eq!(f.cached().len(), 1);
    }

    #[test]
    fn a_gop_beyond_the_bound_is_given_up() {
        let mut f = running();
        f.take(&keyframe(), true);
        for _ in 0..CACHE_MAX {
            f.take(&delta(), true);
        }

        assert!(f.cached().is_empty());

        // and it fills again from the next keyframe
        f.take(&keyframe(), true);
        assert_eq!(f.cached().len(), 1);
    }

    #[test]
    fn the_cache_holds_what_a_new_player_needs() {
        let mut f = running();
        f.take(&keyframe(), true);
        f.take(&delta(), true);

        let cached = f.cached();
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0], keyframe());
        assert_eq!(cached[1], delta());
    }

    #[test]
    fn frames_are_cached_even_while_nobody_listens() {
        let mut f = running();

        assert!(!f.take(&keyframe(), false));
        assert!(!f.take(&delta(), false));

        assert_eq!(f.cached().len(), 2);
        assert_eq!(f.take_stats(), Stats { incoming: 2, dropped: 1, pushed: 0 });
    }

    #[test]
    fn a_restart_drops_the_gop_and_waits_for_a_keyframe() {
        let mut f = running();
        f.take(&keyframe(), true);
        assert!(f.take(&delta(), true));
        assert_eq!(f.cached().len(), 2);

        f.restart();

        assert!(f.cached().is_empty());
        assert!(!f.take(&delta(), true));
        assert!(f.take(&keyframe(), true));
        assert_eq!(f.cached().len(), 1);
    }

    #[test]
    fn the_counters_read_once_and_start_over() {
        let mut f = running();
        f.take(&keyframe(), true);
        f.take(&delta(), true);
        f.take(&delta(), false);

        assert_eq!(f.take_stats(), Stats { incoming: 3, dropped: 1, pushed: 2 });
        assert_eq!(f.take_stats(), Stats::default());
    }

    #[test]
    fn h265_frames_are_classified_as_h265() {
        let mut f = Fanout::new();
        f.set_active(true);
        f.set_codec(CpCodec::H265);
        let mut key = 19u32.to_be_bytes().to_vec();
        key[..4].copy_from_slice(&4u32.to_be_bytes());
        key.push(19 << 1);
        key.extend([0, 0, 0]);

        assert!(f.take(&key, true));
    }

    #[test]
    fn an_empty_frame_is_a_delta() {
        let mut f = running();
        assert!(!f.take(&[], true));
    }
}
