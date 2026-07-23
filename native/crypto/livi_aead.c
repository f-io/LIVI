#include "livi_aead.h"
#include "monocypher.h"
#include <string.h>

static void u64le(uint8_t out[8], uint64_t v) {
  for (int i = 0; i < 8; i++) {
    out[i] = (uint8_t)v;
    v >>= 8;
  }
}

// Poly1305 tag over aad || pad16 || ct || pad16 || le64(aad_len) || le64(ct_len),
// keyed by the ChaCha20 block-0 one-time key.
static void poly_tag(uint8_t mac[16], const uint8_t key[32], const uint8_t nonce[12],
                     const uint8_t *aad, size_t aad_len, const uint8_t *ct, size_t ct_len) {
  uint8_t polykey[32];
  uint8_t zeros[32] = {0};
  crypto_chacha20_ietf(polykey, zeros, sizeof(zeros), key, nonce, 0);

  crypto_poly1305_ctx ctx;
  crypto_poly1305_init(&ctx, polykey);
  static const uint8_t pad[16] = {0};
  crypto_poly1305_update(&ctx, aad, aad_len);
  if (aad_len % 16) crypto_poly1305_update(&ctx, pad, 16 - (aad_len % 16));
  crypto_poly1305_update(&ctx, ct, ct_len);
  if (ct_len % 16) crypto_poly1305_update(&ctx, pad, 16 - (ct_len % 16));
  uint8_t lens[16];
  u64le(lens, (uint64_t)aad_len);
  u64le(lens + 8, (uint64_t)ct_len);
  crypto_poly1305_update(&ctx, lens, sizeof(lens));
  crypto_poly1305_final(&ctx, mac);
  crypto_wipe(polykey, sizeof(polykey));
}

int livi_chacha20poly1305_open(uint8_t *out, size_t *out_len,
                               const uint8_t key[32], const uint8_t nonce[12],
                               const uint8_t *aad, size_t aad_len,
                               const uint8_t *in, size_t in_len) {
  if (in_len < 16) return -1;
  size_t ct_len = in_len - 16;
  const uint8_t *ct = in;
  const uint8_t *tag = in + ct_len;

  uint8_t mac[16];
  poly_tag(mac, key, nonce, aad, aad_len, ct, ct_len);
  if (crypto_verify16(mac, tag) != 0) return -1;

  crypto_chacha20_ietf(out, ct, ct_len, key, nonce, 1);
  *out_len = ct_len;
  return 0;
}

void livi_chacha20poly1305_seal(uint8_t *out, const uint8_t key[32],
                                const uint8_t nonce[12], const uint8_t *aad,
                                size_t aad_len, const uint8_t *pt, size_t pt_len) {
  crypto_chacha20_ietf(out, pt, pt_len, key, nonce, 1);
  poly_tag(out + pt_len, key, nonce, aad, aad_len, out, pt_len);
}
