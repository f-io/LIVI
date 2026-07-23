/**
 * crypto — primitives for the CarPlay Wi-Fi handshake, over node:crypto.
 *
 * Curve25519 (X25519) ECDH, Ed25519 identity sign/verify, HKDF-SHA512 and
 * ChaCha20-Poly1305 AEAD. Node's KeyObjects wrap raw keys in DER, so this also
 * provides raw<->KeyObject helpers since the wire uses bare 32-byte keys.
 *
 * ChaCha20-Poly1305 runs in the livi-crypto native addon: Electron's BoringSSL exposes
 * it only as EVP_AEAD, which node:crypto's createCipheriv (EVP_CIPHER) cannot reach, and
 * a JS AEAD is too slow for the CarPlay video frame rate.
 */

import {
  createCipheriv,
  createHash,
  createPrivateKey,
  createPublicKey,
  diffieHellman,
  sign as edSign,
  verify as edVerify,
  generateKeyPairSync,
  hkdfSync,
  type KeyObject,
  randomBytes
} from 'node:crypto'

// ChaCha20-Poly1305 (RFC 8439) runs in the livi-crypto native addon (Monocypher-backed).
type NativeAead = {
  open(key: Buffer, nonce: Buffer, ct: Buffer, aad?: Buffer): Buffer | null
  seal(key: Buffer, nonce: Buffer, pt: Buffer, aad?: Buffer): Buffer
}
let nativeAead: NativeAead | undefined
function aead(): NativeAead {
  if (nativeAead === undefined) nativeAead = require('livi-crypto') as NativeAead
  return nativeAead
}

// DER wrappers for bare 32-byte X25519/Ed25519 keys.
const X25519_SPKI = Buffer.from('302a300506032b656e032100', 'hex')
const X25519_PKCS8 = Buffer.from('302e020100300506032b656e04220420', 'hex')
const ED25519_SPKI = Buffer.from('302a300506032b6570032100', 'hex')
const ED25519_PKCS8 = Buffer.from('302e020100300506032b657004220420', 'hex')

function rawPub(key: KeyObject): Buffer {
  return key.export({ type: 'spki', format: 'der' }).subarray(-32)
}

function x25519PubFromRaw(raw: Buffer): KeyObject {
  return createPublicKey({ key: Buffer.concat([X25519_SPKI, raw]), format: 'der', type: 'spki' })
}

function x25519PrivFromRaw(raw: Buffer): KeyObject {
  return createPrivateKey({ key: Buffer.concat([X25519_PKCS8, raw]), format: 'der', type: 'pkcs8' })
}

function ed25519PubFromRaw(raw: Buffer): KeyObject {
  return createPublicKey({ key: Buffer.concat([ED25519_SPKI, raw]), format: 'der', type: 'spki' })
}

function ed25519PrivFromRaw(raw: Buffer): KeyObject {
  return createPrivateKey({
    key: Buffer.concat([ED25519_PKCS8, raw]),
    format: 'der',
    type: 'pkcs8'
  })
}

// ── X25519 ────────────────────────────────────────────────────────────────

export interface X25519Pair {
  priv: KeyObject
  pubRaw: Buffer
}

export function x25519Generate(): X25519Pair {
  const { privateKey, publicKey } = generateKeyPairSync('x25519')
  return { priv: privateKey, pubRaw: rawPub(publicKey) }
}

/** ECDH shared secret against a peer's bare 32-byte public key. */
export function x25519Shared(priv: KeyObject, peerPubRaw: Buffer): Buffer {
  return diffieHellman({ privateKey: priv, publicKey: x25519PubFromRaw(peerPubRaw) })
}

export { x25519PrivFromRaw }

// ── Ed25519 ─────────────────────────────────────────────────────────────────

export interface Ed25519Pair {
  privRaw: Buffer
  pubRaw: Buffer
}

export function ed25519Generate(): Ed25519Pair {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519')
  const privRaw = privateKey.export({ type: 'pkcs8', format: 'der' }).subarray(-32)
  return { privRaw, pubRaw: rawPub(publicKey) }
}

export function ed25519Sign(privRaw: Buffer, data: Buffer): Buffer {
  return edSign(null, data, ed25519PrivFromRaw(privRaw))
}

export function ed25519Verify(pubRaw: Buffer, data: Buffer, sig: Buffer): boolean {
  try {
    return edVerify(null, data, ed25519PubFromRaw(pubRaw), sig)
  } catch {
    return false
  }
}

// ── HKDF-SHA512 ─────────────────────────────────────────────────────────────

export function hkdfSha512(
  ikm: Buffer,
  salt: string | Buffer,
  info: string | Buffer,
  length = 32
): Buffer {
  const out = hkdfSync(
    'sha512',
    ikm,
    typeof salt === 'string' ? Buffer.from(salt) : salt,
    typeof info === 'string' ? Buffer.from(info) : info,
    length
  )
  return Buffer.from(out)
}

// ── ChaCha20-Poly1305 AEAD ──────────────────────────────────────────────────

/** Seal: returns ciphertext concatenated with the 16-byte auth tag. */
export function chachaSeal(key: Buffer, nonce: Buffer, plaintext: Buffer, aad?: Buffer): Buffer {
  return aead().seal(key, nonce, plaintext, aad)
}

/** Open: input is ciphertext concatenated with the 16-byte tag. Throws on auth failure. */
export function chachaOpen(key: Buffer, nonce: Buffer, data: Buffer, aad?: Buffer): Buffer {
  const out = aead().open(key, nonce, data, aad)
  if (!out) throw new Error('chacha20poly1305: authentication failed')
  return out
}

/** 12-byte nonce: 4 zero bytes + an 8-byte little-endian counter (AirPlay style). */
export function nonce64(counter: bigint): Buffer {
  const n = Buffer.alloc(12)
  n.writeBigUInt64LE(counter, 4)
  return n
}

/** 12-byte nonce from an 8-byte ASCII label right-aligned (HomeKit pairing style, e.g. "PV-Msg02"). */
export function nonceLabel(label: string): Buffer {
  const n = Buffer.alloc(12)
  Buffer.from(label, 'ascii').copy(n, 4)
  return n
}

export function randomId(): string {
  const b = randomBytes(6)
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join(':')
}

// ── SHA + AES-CTR (MFiSAP /auth-setup) ──────────────────────────────────────

export function sha1(...parts: Buffer[]): Buffer {
  const h = createHash('sha1')
  for (const p of parts) h.update(p)
  return h.digest()
}

export function sha256(...parts: Buffer[]): Buffer {
  const h = createHash('sha256')
  for (const p of parts) h.update(p)
  return h.digest()
}

/** AES-128 in counter mode (MFiSAP signature encryption). */
export function aesCtr128(key: Buffer, iv: Buffer, data: Buffer): Buffer {
  const c = createCipheriv('aes-128-ctr', key, iv)
  return Buffer.concat([c.update(data), c.final()])
}
