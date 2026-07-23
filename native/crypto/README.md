# native/crypto (livi-crypto)

N-API addon that gives the main process native ChaCha20-Poly1305, used for the
CarPlay handshake and to decrypt screen video frames without the JS AEAD's CPU
cost. One N-API build loads under both Electron and the test runner, so it must
be built (via `install-app-deps` or `build:native:*`) before either runs.

Exports `open(key, nonce, ct, aad?)` and `seal(key, nonce, pt, aad?)`.

## livi_aead

`livi_aead.c` / `livi_aead.h` implement RFC 8439 ChaCha20-Poly1305 (12-byte
nonce) open and seal on top of Monocypher's IETF ChaCha20 and Poly1305
primitives. Monocypher's own `crypto_aead_*` is XChaCha20 (24-byte nonce) and
does not match the wire format, so the construction is built here.

## Monocypher (vendored)

`monocypher.c` / `monocypher.h` are vendored verbatim from Monocypher 4.0.2.

- Upstream: https://github.com/LoupVaillant/Monocypher
- Files: `src/monocypher.c`, `src/monocypher.h`
- Licence: dual CC0-1.0 / BSD-2-Clause, see `LICENCE.md`

To update, replace both files from the chosen release tag and update this note.
