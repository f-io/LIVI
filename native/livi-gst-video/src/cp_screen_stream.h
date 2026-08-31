#ifndef CP_SCREEN_STREAM_H
#define CP_SCREEN_STREAM_H

#include <stddef.h>
#include <stdint.h>

// Framing and decryption of the CarPlay screen stream, implemented by the
// livi-screen-stream crate (rust/screen). The caller supplies the bytes; the
// callbacks fire on the pushing thread.

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
  void (*on_config)(int codec, const uint8_t* atom, size_t len, void* user);
  void (*on_frame)(const uint8_t* nal, size_t len, void* user);
  void (*on_started)(void* user);
  void* user;
} CpScreenCallbacks;

typedef struct CpScreenStream CpScreenStream;

CpScreenStream* cp_screen_stream_new(const uint8_t key[32], const CpScreenCallbacks* cb);

void cp_screen_stream_reset(CpScreenStream* s);

// 0 to read on, -1 when the stream is broken and the connection must go.
int cp_screen_stream_push(CpScreenStream* s, const uint8_t* data, size_t len);

void cp_screen_stream_free(CpScreenStream* s);

#ifdef __cplusplus
}
#endif

#endif
