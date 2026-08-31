#ifndef CP_PLAYER_H
#define CP_PLAYER_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// One decoded stream, implemented by the livi-video-player crate (rust/player).

#ifdef __cplusplus
extern "C" {
#endif

typedef struct CpPlayer CpPlayer;

void cp_gst_ensure_init(void);
void cp_gst_probe(const char* codec, bool* hw, bool* sw);

CpPlayer* cp_player_new(const char* codec, uintptr_t handle, const uint8_t* codec_data,
                        size_t codec_data_len);
void cp_player_free(CpPlayer* p);
void cp_player_start(CpPlayer* p);
bool cp_player_push(CpPlayer* p, const uint8_t* data, size_t len);
void cp_player_set_gamma(CpPlayer* p, double gamma, double contrast, double gain_r, double gain_g,
                         double gain_b);

#ifdef __cplusplus
}
#endif

#endif
