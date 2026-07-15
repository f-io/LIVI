/**
 * pairVerify — the CarPlay pair-verify responder (accessory/server side).
 *
 * A reconnecting phone proves it holds a controller LTPK stored during
 * pair-setup, using ephemeral X25519 + Ed25519 signatures (HAP pair-verify).
 * On success it yields the ChaCha20-Poly1305 control-channel keys and the
 * accessory switches the connection to encrypted framing. TLV8 over
 * "application/pairing+tlv8".
 */

import {
  chachaOpen,
  chachaSeal,
  ed25519Sign,
  ed25519Verify,
  hkdfSha512,
  nonceLabel,
  x25519Generate,
  x25519Shared
} from './crypto'
import { loadOrCreateIdentity } from './identity'
import { getPairing } from './pairings'
import { decodeTlv8, encodeTlv8 } from './tlv8'

const TLV = {
  Identifier: 0x01,
  PublicKey: 0x03,
  EncryptedData: 0x05,
  State: 0x06,
  Error: 0x07,
  Signature: 0x0a
} as const

const ERR_AUTHENTICATION = 2

/** Per-direction ChaCha20-Poly1305 keys for the control channel. */
export interface ControlKeys {
  readKey: Buffer
  writeKey: Buffer
}

export class PairVerify {
  private ephPub: Buffer | null = null
  private clientEphPub: Buffer | null = null
  private shared: Buffer | null = null
  private encKey: Buffer | null = null
  private _keys: ControlKeys | null = null
  private _verified = false
  private _controllerId: string | null = null

  get verified(): boolean {
    return this._verified
  }

  get controllerId(): string | null {
    return this._controllerId
  }

  get controlKeys(): ControlKeys | null {
    return this._keys
  }

  /** The pair-verify X25519 shared secret, used to derive per-stream keys. */
  get sharedSecret(): Buffer | null {
    return this.shared
  }

  /** Handle one pair-verify TLV8 request, return the TLV8 response body. */
  handle(body: Buffer): Buffer {
    const tlv = decodeTlv8(body)
    const state = tlv.get(TLV.State)?.readUInt8(0)
    try {
      if (state === 1) return this.m2(tlv)
      if (state === 3) return this.m4(tlv)
    } catch (e) {
      console.warn('[pairVerify] error:', (e as Error).message)
    }
    return this.err(state ?? 0)
  }

  private m2(tlv: Map<number, Buffer>): Buffer {
    const clientEphPub = tlv.get(TLV.PublicKey)
    if (!clientEphPub) return this.err(2)

    const eph = x25519Generate()
    this.ephPub = eph.pubRaw
    this.clientEphPub = clientEphPub
    this.shared = x25519Shared(eph.priv, clientEphPub)
    this.encKey = hkdfSha512(
      this.shared,
      'Pair-Verify-Encrypt-Salt',
      'Pair-Verify-Encrypt-Info',
      32
    )

    // Sign ownEphPub || ownPairingId || peerEphPub with the accessory LTSK.
    const id = loadOrCreateIdentity()
    const accId = Buffer.from(id.pairingId, 'utf8')
    const sig = ed25519Sign(id.privRaw, Buffer.concat([this.ephPub, accId, clientEphPub]))
    const sub = encodeTlv8([
      { type: TLV.Identifier, value: accId },
      { type: TLV.Signature, value: sig }
    ])
    const sealed = chachaSeal(this.encKey, nonceLabel('PV-Msg02'), sub)
    console.log('[pairVerify] M1->M2')
    return encodeTlv8([
      { type: TLV.State, value: Buffer.from([2]) },
      { type: TLV.PublicKey, value: this.ephPub },
      { type: TLV.EncryptedData, value: sealed }
    ])
  }

  private m4(tlv: Map<number, Buffer>): Buffer {
    const enc = tlv.get(TLV.EncryptedData)
    if (!this.encKey || !this.shared || !this.ephPub || !this.clientEphPub || !enc)
      return this.err(4)

    const sub = decodeTlv8(chachaOpen(this.encKey, nonceLabel('PV-Msg03'), enc))
    const ctrlId = sub.get(TLV.Identifier)
    const ctrlSig = sub.get(TLV.Signature)
    if (!ctrlId || !ctrlSig) return this.err(4)

    const ctrlLtpk = getPairing(ctrlId.toString('utf8'))
    if (!ctrlLtpk) {
      console.warn(`[pairVerify] unknown controller ${ctrlId.toString('utf8')}`)
      return this.err(4)
    }
    // Controller signed peerEphPub(ours) || ownPairingId || ownEphPub(theirs).
    const sigData = Buffer.concat([this.clientEphPub, ctrlId, this.ephPub])
    if (!ed25519Verify(ctrlLtpk, sigData, ctrlSig)) {
      console.warn('[pairVerify] controller signature invalid')
      return this.err(4)
    }

    // Control-channel keys. Key names are from the controller's view, so the
    // accessory reads with the controller's WRITE key and writes with its READ key.
    this._keys = {
      readKey: hkdfSha512(this.shared, 'Control-Salt', 'Control-Write-Encryption-Key', 32),
      writeKey: hkdfSha512(this.shared, 'Control-Salt', 'Control-Read-Encryption-Key', 32)
    }
    this._verified = true
    this._controllerId = ctrlId.toString('utf8')
    console.log(`[pairVerify] M3->M4 verified (controller ${this._controllerId})`)
    return encodeTlv8([{ type: TLV.State, value: Buffer.from([4]) }])
  }

  private err(state: number): Buffer {
    return encodeTlv8([
      { type: TLV.State, value: Buffer.from([state]) },
      { type: TLV.Error, value: Buffer.from([ERR_AUTHENTICATION]) }
    ])
  }
}
