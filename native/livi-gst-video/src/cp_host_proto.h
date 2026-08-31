#ifndef CP_HOST_PROTO_H
#define CP_HOST_PROTO_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Framing of the gst-host control protocol, implemented by the livi-host-proto
// crate (rust/hostproto).

#ifdef __cplusplus
extern "C" {
#endif

typedef struct CpHostFramer CpHostFramer;

CpHostFramer* cp_host_framer_new(void);
void cp_host_framer_free(CpHostFramer* f);
void cp_host_framer_push(CpHostFramer* f, const uint8_t* data, size_t len);

// Takes the next message; false while one is still arriving. rest stays valid
// until the next call.
bool cp_host_framer_next(CpHostFramer* f, uint8_t* op, uint32_t* id, const uint8_t** rest,
                         size_t* rest_len);

// Writes the reply into out and returns its length, 0 when it does not fit.
size_t cp_host_reply_encode(uint8_t op, uint32_t id, const uint8_t* rest, size_t rlen, uint8_t* out,
                            size_t cap);

#ifdef __cplusplus
}
#endif

#endif
