/**
 * pairSetup — the CarPlay pair-setup responder (accessory/server side).
 *
 * CarPlay uses "unauthenticated" pair-setup: plain SRP-6a (3072-bit, SHA-512)
 * with the fixed PIN "3939" (no user entry, no MFi inside pair-setup), followed
 * by an encrypted Ed25519 long-term-key exchange (M5/M6). The controller's LTPK
 * is persisted for later pair-verify. Per HAP TLV8 over "application/pairing+tlv8".
 */

import {
  chachaOpen,
  chachaSeal,
  ed25519Sign,
  ed25519Verify,
  hkdfSha512,
  nonceLabel
} from './crypto'
import { loadOrCreateIdentity } from './identity'
import { savePairing } from './pairings'
import { type SrpServer, srpStartServer } from './srp'
import { decodeTlv8, encodeTlv8 } from './tlv8'

const TLV = {
  Method: 0x00,
  Identifier: 0x01,
  Salt: 0x02,
  PublicKey: 0x03,
  Proof: 0x04,
  EncryptedData: 0x05,
  State: 0x06,
  Error: 0x07,
  Signature: 0x0a
} as const

const SETUP_CODE = '3939'
const ERR_AUTHENTICATION = 2

export class PairSetup {
  private srp: SrpServer | null = null
  private K: Buffer | null = null
  private _complete = false

  get complete(): boolean {
    return this._complete
  }

  /** Handle one pair-setup TLV8 request, return the TLV8 response body. */
  handle(body: Buffer): Buffer {
    const tlv = decodeTlv8(body)
    const state = tlv.get(TLV.State)?.readUInt8(0)
    try {
      if (state === 1) return this.m2()
      if (state === 3) return this.m4(tlv)
      if (state === 5) return this.m6(tlv)
    } catch (e) {
      console.warn('[pairSetup] error:', (e as Error).message)
    }
    return this.err(state ?? 0)
  }

  private m2(): Buffer {
    this.srp = srpStartServer('Pair-Setup', SETUP_CODE)
    console.log('[pairSetup] M1->M2 (SRP start)')
    return encodeTlv8([
      { type: TLV.State, value: Buffer.from([2]) },
      { type: TLV.PublicKey, value: this.srp.B },
      { type: TLV.Salt, value: this.srp.salt }
    ])
  }

  private m4(tlv: Map<number, Buffer>): Buffer {
    const A = tlv.get(TLV.PublicKey)
    const proof = tlv.get(TLV.Proof)
    if (!this.srp || !A || !proof) return this.err(4)
    const r = this.srp.verify(A, proof)
    if (!r.ok || !r.K || !r.serverM2) {
      console.warn('[pairSetup] SRP verify failed')
      return this.err(4)
    }
    this.K = r.K
    console.log('[pairSetup] M3->M4 (SRP verified)')
    return encodeTlv8([
      { type: TLV.State, value: Buffer.from([4]) },
      { type: TLV.Proof, value: r.serverM2 }
    ])
  }

  private m6(tlv: Map<number, Buffer>): Buffer {
    const enc = tlv.get(TLV.EncryptedData)
    if (!this.K || !enc) return this.err(6)

    const encKey = hkdfSha512(this.K, 'Pair-Setup-Encrypt-Salt', 'Pair-Setup-Encrypt-Info', 32)
    const sub = decodeTlv8(chachaOpen(encKey, nonceLabel('PS-Msg05'), enc))
    const ctrlId = sub.get(TLV.Identifier)
    const ctrlLtpk = sub.get(TLV.PublicKey)
    const ctrlSig = sub.get(TLV.Signature)
    if (!ctrlId || !ctrlLtpk || !ctrlSig) return this.err(6)

    // Verify the controller signed (signKey || identifier || its LTPK).
    const ctrlSignKey = hkdfSha512(
      this.K,
      'Pair-Setup-Controller-Sign-Salt',
      'Pair-Setup-Controller-Sign-Info',
      32
    )
    const ctrlSignData = Buffer.concat([ctrlSignKey, ctrlId, ctrlLtpk])
    if (!ed25519Verify(ctrlLtpk, ctrlSignData, ctrlSig)) {
      console.warn('[pairSetup] controller signature invalid')
      return this.err(6)
    }
    savePairing(ctrlId.toString('utf8'), ctrlLtpk)

    // Build our M6: accessory identifier + LTPK + signature.
    const id = loadOrCreateIdentity()
    const accId = Buffer.from(id.pairingId, 'utf8')
    const accSignKey = hkdfSha512(
      this.K,
      'Pair-Setup-Accessory-Sign-Salt',
      'Pair-Setup-Accessory-Sign-Info',
      32
    )
    const accSignData = Buffer.concat([accSignKey, accId, id.pubRaw])
    const accSig = ed25519Sign(id.privRaw, accSignData)
    const subResp = encodeTlv8([
      { type: TLV.Identifier, value: accId },
      { type: TLV.PublicKey, value: id.pubRaw },
      { type: TLV.Signature, value: accSig }
    ])
    const sealed = chachaSeal(encKey, nonceLabel('PS-Msg06'), subResp)
    this._complete = true
    console.log(`[pairSetup] M5->M6 paired (controller ${ctrlId.toString('utf8')})`)
    return encodeTlv8([
      { type: TLV.State, value: Buffer.from([6]) },
      { type: TLV.EncryptedData, value: sealed }
    ])
  }

  private err(state: number): Buffer {
    return encodeTlv8([
      { type: TLV.State, value: Buffer.from([state]) },
      { type: TLV.Error, value: Buffer.from([ERR_AUTHENTICATION]) }
    ])
  }
}
