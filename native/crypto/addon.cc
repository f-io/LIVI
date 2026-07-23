#include <node_api.h>
#include "livi_aead.h"

// open(key: Buffer(32), nonce: Buffer(12), ct: Buffer(>=16), aad?: Buffer)
//   -> Buffer(plaintext) on a valid tag, null on auth failure or bad arguments.
// ct is ciphertext followed by the 16-byte Poly1305 tag.
static napi_value Open(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4];
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  napi_value null_value;
  napi_get_null(env, &null_value);

  uint8_t *key = nullptr, *nonce = nullptr, *ct = nullptr, *aad = nullptr;
  size_t klen = 0, nlen = 0, clen = 0, alen = 0;
  if (argc < 3) return null_value;
  if (napi_get_buffer_info(env, argv[0], (void**)&key, &klen) != napi_ok || klen != 32)
    return null_value;
  if (napi_get_buffer_info(env, argv[1], (void**)&nonce, &nlen) != napi_ok || nlen != 12)
    return null_value;
  if (napi_get_buffer_info(env, argv[2], (void**)&ct, &clen) != napi_ok || clen < 16)
    return null_value;
  if (argc >= 4) {
    napi_valuetype t;
    napi_typeof(env, argv[3], &t);
    if (t != napi_undefined && t != napi_null &&
        napi_get_buffer_info(env, argv[3], (void**)&aad, &alen) != napi_ok)
      return null_value;
  }

  void* out = nullptr;
  napi_value out_buffer;
  if (napi_create_buffer(env, clen - 16, &out, &out_buffer) != napi_ok) return null_value;

  size_t out_len = 0;
  if (livi_chacha20poly1305_open((uint8_t*)out, &out_len, key, nonce, aad, alen, ct, clen) != 0)
    return null_value;
  return out_buffer;
}

// seal(key: Buffer(32), nonce: Buffer(12), pt: Buffer, aad?: Buffer)
//   -> Buffer(ciphertext + 16-byte tag), or null on bad arguments.
static napi_value Seal(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4];
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  napi_value null_value;
  napi_get_null(env, &null_value);

  uint8_t *key = nullptr, *nonce = nullptr, *pt = nullptr, *aad = nullptr;
  size_t klen = 0, nlen = 0, plen = 0, alen = 0;
  if (argc < 3) return null_value;
  if (napi_get_buffer_info(env, argv[0], (void**)&key, &klen) != napi_ok || klen != 32)
    return null_value;
  if (napi_get_buffer_info(env, argv[1], (void**)&nonce, &nlen) != napi_ok || nlen != 12)
    return null_value;
  if (napi_get_buffer_info(env, argv[2], (void**)&pt, &plen) != napi_ok)
    return null_value;
  if (argc >= 4) {
    napi_valuetype t;
    napi_typeof(env, argv[3], &t);
    if (t != napi_undefined && t != napi_null &&
        napi_get_buffer_info(env, argv[3], (void**)&aad, &alen) != napi_ok)
      return null_value;
  }

  void* out = nullptr;
  napi_value out_buffer;
  if (napi_create_buffer(env, plen + 16, &out, &out_buffer) != napi_ok) return null_value;
  livi_chacha20poly1305_seal((uint8_t*)out, key, nonce, aad, alen, pt, plen);
  return out_buffer;
}

static napi_value Init(napi_env env, napi_value exports) {
  napi_value fn;
  napi_create_function(env, "open", NAPI_AUTO_LENGTH, Open, nullptr, &fn);
  napi_set_named_property(env, exports, "open", fn);
  napi_create_function(env, "seal", NAPI_AUTO_LENGTH, Seal, nullptr, &fn);
  napi_set_named_property(env, exports, "seal", fn);
  return exports;
}

NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
