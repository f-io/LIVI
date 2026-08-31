//! Which frames a receiver lets through, and the GOP it keeps.
//!
//! A receiver drops everything until it has seen a keyframe, so a decoder never
//! starts on a delta frame. From every keyframe on it keeps the frames that
//! follow, so a player created mid-stream can be fed the running GOP instead of
//! waiting for the next keyframe. The cache is dropped when it outgrows its
//! bound, because a stream without keyframes would otherwise grow without end.

use livi_video_nal::{classify_nal, CpCodec, CpNalKind};

/// Frames kept at most; beyond this the GOP is given up.
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

    /// Drops everything until the next keyframe and gives up the cached GOP,
    /// so a player primed from here never sees frames of the old stream.
    pub fn restart(&mut self) {
        self.awaiting_keyframe = true;
        self.cache.clear();
        self.cache_valid = false;
    }

    pub fn awaiting_keyframe(&self) -> bool {
        self.awaiting_keyframe
    }

    /// The frames a player created now would be fed.
    pub fn cached(&self) -> &[Vec<u8>] {
        if self.cache_valid { &self.cache } else { &[] }
    }

    /// Reads the counters and starts them over.
    pub fn take_stats(&mut self) -> Stats {
        core::mem::take(&mut self.stats)
    }

    /// Accounts for `nal` and answers whether the caller pushes it. `has_target`
    /// says whether any player is listening; the frame still enters the cache
    /// when none is.
    pub fn take(&mut self, nal: &[u8], has_target: bool) -> bool {
        self.stats.incoming += 1;
        if !self.active {
            return false;
        }
        if !self.have_codec {
            self.stats.dropped += 1;
            return false;
        }

        let kind = classify_nal(nal, self.codec);
        if self.awaiting_keyframe {
            self.stats.dropped += 1;
            match kind {
                // parameter sets ride along, they are what the keyframe needs
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

        if has_target {
            self.stats.pushed += 1;
        }
        has_target
    }
}

/// # Safety
/// The returned fanout is owned by the caller and freed with `cp_fanout_free`.
#[unsafe(no_mangle)]
pub extern "C" fn cp_fanout_new() -> *mut Fanout {
    Box::into_raw(Box::new(Fanout::new()))
}

/// # Safety
/// `f` comes from `cp_fanout_new` and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_free(f: *mut Fanout) {
    if !f.is_null() {
        drop(unsafe { Box::from_raw(f) });
    }
}

/// # Safety
/// `f` comes from `cp_fanout_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_set_active(f: *mut Fanout, active: bool) {
    if let Some(f) = unsafe { f.as_mut() } {
        f.set_active(active);
    }
}

/// # Safety
/// `f` comes from `cp_fanout_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_is_active(f: *const Fanout) -> bool {
    unsafe { f.as_ref() }.is_some_and(|f| f.is_active())
}

/// # Safety
/// `f` comes from `cp_fanout_new`; `codec` is a `CpCodec` value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_set_codec(f: *mut Fanout, codec: i32) {
    if let Some(f) = unsafe { f.as_mut() } {
        f.set_codec(if codec == CpCodec::H265 as i32 { CpCodec::H265 } else { CpCodec::H264 });
    }
}

/// # Safety
/// `f` comes from `cp_fanout_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_restart(f: *mut Fanout) {
    if let Some(f) = unsafe { f.as_mut() } {
        f.restart();
    }
}

/// Answers whether the caller pushes this frame.
///
/// # Safety
/// `f` comes from `cp_fanout_new`, `nal` points to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_take(
    f: *mut Fanout,
    nal: *const u8,
    len: usize,
    has_target: bool,
) -> bool {
    let Some(f) = (unsafe { f.as_mut() }) else {
        return false;
    };
    let bytes = if nal.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(nal, len) } };
    f.take(bytes, has_target)
}

/// How many frames the cache holds for priming a new player.
///
/// # Safety
/// `f` comes from `cp_fanout_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_cache_len(f: *const Fanout) -> usize {
    unsafe { f.as_ref() }.map_or(0, |f| f.cached().len())
}

/// Points `out` at cached frame `i` and returns its length, 0 when there is none.
///
/// # Safety
/// `f` comes from `cp_fanout_new`; the frame stays valid until the next
/// `cp_fanout_take`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_cached(
    f: *const Fanout,
    i: usize,
    out: *mut *const u8,
) -> usize {
    let Some(f) = (unsafe { f.as_ref() }) else {
        return 0;
    };
    match f.cached().get(i) {
        Some(frame) => {
            unsafe { *out = frame.as_ptr() };
            frame.len()
        }
        None => 0,
    }
}

/// Reads the counters and starts them over. Returns true when any was non-zero.
///
/// # Safety
/// `f` comes from `cp_fanout_new`; the out pointers are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_fanout_take_stats(
    f: *mut Fanout,
    incoming: *mut u64,
    dropped: *mut u64,
    pushed: *mut u64,
    awaiting_keyframe: *mut bool,
    active: *mut bool,
) -> bool {
    let Some(f) = (unsafe { f.as_mut() }) else {
        return false;
    };
    let awaiting = f.awaiting_keyframe();
    let is_active = f.is_active();
    let s = f.take_stats();
    unsafe {
        *incoming = s.incoming;
        *dropped = s.dropped;
        *pushed = s.pushed;
        *awaiting_keyframe = awaiting;
        *active = is_active;
    }
    s.incoming != 0 || s.dropped != 0 || s.pushed != 0
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
    fn an_inactive_receiver_counts_the_frame_and_nothing_else() {
        let mut f = Fanout::new();
        f.set_codec(CpCodec::H264);

        assert!(!f.take(&keyframe(), true));
        assert_eq!(f.take_stats(), Stats { incoming: 1, dropped: 0, pushed: 0 });
        assert!(f.cached().is_empty());
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
    fn the_c_entry_points_answer_like_the_safe_ones() {
        let f = cp_fanout_new();
        unsafe {
            cp_fanout_set_active(f, true);
            assert!(cp_fanout_is_active(f));
            cp_fanout_set_codec(f, CpCodec::H264 as i32);

            let d = delta();
            assert!(!cp_fanout_take(f, d.as_ptr(), d.len(), true));
            let k = keyframe();
            assert!(cp_fanout_take(f, k.as_ptr(), k.len(), true));

            assert_eq!(cp_fanout_cache_len(f), 1);
            let mut out = core::ptr::null();
            let len = cp_fanout_cached(f, 0, &mut out);
            assert_eq!(core::slice::from_raw_parts(out, len), &k[..]);
            assert_eq!(cp_fanout_cached(f, 9, &mut out), 0);

            let (mut i, mut d2, mut p) = (0u64, 0u64, 0u64);
            let (mut aw, mut ac) = (false, false);
            assert!(cp_fanout_take_stats(f, &mut i, &mut d2, &mut p, &mut aw, &mut ac));
            assert_eq!((i, d2, p, aw, ac), (2, 2, 1, false, true));
            assert!(!cp_fanout_take_stats(f, &mut i, &mut d2, &mut p, &mut aw, &mut ac));

            cp_fanout_restart(f);
            assert!(!cp_fanout_take(f, d.as_ptr(), d.len(), true));
            cp_fanout_free(f);
        }
    }

    #[test]
    fn a_null_fanout_stays_quiet() {
        let null: *mut Fanout = core::ptr::null_mut();
        unsafe {
            cp_fanout_set_active(null, true);
            cp_fanout_set_codec(null, 0);
            cp_fanout_restart(null);
            cp_fanout_free(null);
            assert!(!cp_fanout_take(null, core::ptr::null(), 0, true));
            assert!(!cp_fanout_is_active(core::ptr::null()));
            assert_eq!(cp_fanout_cache_len(core::ptr::null()), 0);
            let mut out = core::ptr::null();
            assert_eq!(cp_fanout_cached(core::ptr::null(), 0, &mut out), 0);
            let (mut i, mut d, mut p) = (0u64, 0u64, 0u64);
            let (mut aw, mut ac) = (false, false);
            assert!(!cp_fanout_take_stats(null, &mut i, &mut d, &mut p, &mut aw, &mut ac));
        }
    }

    #[test]
    fn an_empty_frame_is_a_delta() {
        let mut f = running();
        assert!(!f.take(&[], true));
    }
}
