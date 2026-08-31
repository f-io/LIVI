#ifndef CP_VIDEO_FANOUT_H
#define CP_VIDEO_FANOUT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Per-receiver frame gating and GOP cache, implemented by the
// livi-video-fanout crate (rust/fanout). The caller owns the planes and does
// the pushing; this decides what is worth pushing and what a player created
// mid-stream has to be fed first.

#ifdef __cplusplus
extern "C" {
#endif

typedef struct CpFanout CpFanout;

CpFanout* cp_fanout_new(void);
void cp_fanout_free(CpFanout* f);

void cp_fanout_set_active(CpFanout* f, bool active);
bool cp_fanout_is_active(const CpFanout* f);
void cp_fanout_set_codec(CpFanout* f, int codec);

// Drops the cached GOP and waits for the next keyframe.
void cp_fanout_restart(CpFanout* f);

// True when the caller pushes this frame to its targets.
bool cp_fanout_take(CpFanout* f, const uint8_t* nal, size_t len, bool has_target);

size_t cp_fanout_cache_len(const CpFanout* f);
size_t cp_fanout_cached(const CpFanout* f, size_t i, const uint8_t** out);

// Reads the window counters and starts them over; true when any was non-zero.
bool cp_fanout_take_stats(CpFanout* f, uint64_t* incoming, uint64_t* dropped, uint64_t* pushed,
                          bool* awaiting_keyframe, bool* active);

#ifdef __cplusplus
}
#endif

#endif
