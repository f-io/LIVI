#ifndef CP_VIDEO_CODEC_H
#define CP_VIDEO_CODEC_H

#include <stdbool.h>
#include <stddef.h>

// Element choice per codec, implemented by the livi-video-codec crate
// (rust/codec). The candidate list is ordered best first; the caller takes the
// first name the GStreamer registry knows.

#ifdef __cplusplus
extern "C" {
#endif

const char* cp_parser_for(const char* codec);

const char* cp_caps_for(const char* codec);

const char* cp_sw_decoder_for(const char* codec);

bool cp_is_hw_decoder(const char* name);

// Writes up to cap names into out and returns how many; the pointers are static.
size_t cp_decoder_candidates(const char* codec, bool sw_only, const char** out, size_t cap);

bool cp_has_calibration(void);

// The whole pipeline description; free the result with cp_string_free.
char* cp_pipeline_desc(const char* codec, const char* decoder, bool with_cal);

void cp_string_free(char* s);

#ifdef __cplusplus
}
#endif

#endif
