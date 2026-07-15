/**
 * identity — the accessory's persistent CarPlay/AirPlay-2 identity.
 *
 * A long-term Ed25519 key pair plus a stable pairing identifier. The public key
 * (hex) is advertised as the `pk` TXT record and the pairing id as `pi`; the
 * private key signs the pair-verify proof. Persisted in userData so the phone
 * stays paired across restarts.
 */

import { randomUUID } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'
import { ed25519Generate } from './crypto'

export interface CpIdentity {
  privRaw: Buffer
  pubRaw: Buffer
  /** `pi` TXT value. */
  pairingId: string
  /** `pk` TXT value = lowercase hex of the Ed25519 public key. */
  pkHex: string
}

let cached: CpIdentity | null = null

function identityFile(): string {
  return join(app.getPath('userData'), 'cp', 'identity.json')
}

export function loadOrCreateIdentity(): CpIdentity {
  if (cached) return cached

  const file = identityFile()
  try {
    if (existsSync(file)) {
      const j = JSON.parse(readFileSync(file, 'utf8')) as { priv: string; pub: string; pi: string }
      const privRaw = Buffer.from(j.priv, 'hex')
      const pubRaw = Buffer.from(j.pub, 'hex')
      cached = { privRaw, pubRaw, pairingId: j.pi, pkHex: pubRaw.toString('hex') }
      return cached
    }
  } catch {
    /* fall through and regenerate */
  }

  const kp = ed25519Generate()
  const pairingId = randomUUID()
  try {
    mkdirSync(join(app.getPath('userData'), 'cp'), { recursive: true })
    writeFileSync(
      file,
      JSON.stringify({
        priv: kp.privRaw.toString('hex'),
        pub: kp.pubRaw.toString('hex'),
        pi: pairingId
      }),
      { mode: 0o600 }
    )
  } catch (e) {
    console.warn('[cpIdentity] could not persist identity:', (e as Error).message)
  }

  cached = {
    privRaw: kp.privRaw,
    pubRaw: kp.pubRaw,
    pairingId,
    pkHex: kp.pubRaw.toString('hex')
  }
  return cached
}
