/**
 * authSetup — the CarPlay /auth-setup MFiSAP responder (accessory side).
 *
 * Proves genuine MFi licensing to the phone. Request is raw binary
 * [1B version=1][32B controller X25519 pub]; the accessory replies with its own
 * ephemeral X25519 pub, its MFi certificate, and its coprocessor signature over
 * the two public keys, with the signature AES-128-CTR encrypted under a key
 * derived from the shared secret. Runs over the already-encrypted control
 * channel.
 */

import { aesCtr128, sha1, sha256, x25519Generate, x25519Shared } from './crypto'
import type { MfiSigner } from './mfiSigner'

const MFISAP_VERSION = 0x01

/**
 * Handle one /auth-setup request. Returns the raw binary response body, or null
 * if the request is malformed (caller answers with an error status).
 */
export async function handleAuthSetup(body: Buffer, signer: MfiSigner): Promise<Buffer | null> {
  if (body.length !== 33 || body[0] !== MFISAP_VERSION) {
    console.warn(`[authSetup] bad request (len ${body.length}, ver ${body[0]})`)
    return null
  }
  const peerPub = body.subarray(1, 33)

  const eph = x25519Generate()
  const ourPub = eph.pubRaw
  const shared = x25519Shared(eph.priv, peerPub)
  if (shared.every((b) => b === 0)) {
    console.warn('[authSetup] invalid shared secret')
    return null
  }

  const aesKey = sha1(Buffer.from('AES-KEY'), shared).subarray(0, 16)
  const aesIv = sha1(Buffer.from('AES-IV'), shared).subarray(0, 16)

  const cert = await signer.certificate()
  // Short certs (MFi 3.0) sign a SHA-256 digest; longer ones (2.0) use SHA-1.
  const digest = cert.length <= 640 ? sha256(ourPub, peerPub) : sha1(ourPub, peerPub)
  const sig = await signer.sign(digest)
  const encSig = aesCtr128(aesKey, aesIv, sig)

  const certLen = Buffer.alloc(4)
  certLen.writeUInt32BE(cert.length, 0)
  const sigLen = Buffer.alloc(4)
  sigLen.writeUInt32BE(encSig.length, 0)

  console.log(
    `[authSetup] signed (cert ${cert.length}B, sig ${encSig.length}B, sha${digest.length === 32 ? 256 : 1})`
  )
  return Buffer.concat([ourPub, certLen, cert, sigLen, encSig])
}
