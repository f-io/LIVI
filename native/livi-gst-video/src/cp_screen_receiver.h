#ifndef CP_SCREEN_RECEIVER_H
#define CP_SCREEN_RECEIVER_H

#include <stddef.h>
#include <stdint.h>

#include "cp_screen_stream.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct CpScreenReceiver CpScreenReceiver;

CpScreenReceiver* cp_screen_receiver_new(const uint8_t key[32], const CpScreenCallbacks* cb,
                                         uint16_t* out_port);

void cp_screen_receiver_free(CpScreenReceiver* r);

#ifdef __cplusplus
}
#endif

#endif
