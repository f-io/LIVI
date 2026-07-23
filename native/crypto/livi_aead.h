#ifndef LIVI_AEAD_H
#define LIVI_AEAD_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// RFC 8439 ChaCha20-Poly1305 (12-byte nonce) open, built on Monocypher's IETF
// ChaCha20 and Poly1305 primitives. `in` is ciphertext followed by the 16-byte
// tag; `out` must hold in_len - 16 bytes. Returns 0 and sets *out_len on a valid
// tag, -1 on authentication failure or an input shorter than the tag.
int livi_chacha20poly1305_open(uint8_t *out, size_t *out_len,
                               const uint8_t key[32], const uint8_t nonce[12],
                               const uint8_t *aad, size_t aad_len,
                               const uint8_t *in, size_t in_len);

// RFC 8439 ChaCha20-Poly1305 seal. `out` must hold pt_len + 16 bytes and receives
// the ciphertext followed by the 16-byte Poly1305 tag.
void livi_chacha20poly1305_seal(uint8_t *out, const uint8_t key[32],
                                const uint8_t nonce[12], const uint8_t *aad,
                                size_t aad_len, const uint8_t *pt, size_t pt_len);

#ifdef __cplusplus
}
#endif

#endif
