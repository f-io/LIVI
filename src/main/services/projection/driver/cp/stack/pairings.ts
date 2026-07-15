/**
 * pairings — persistent store of paired controllers (phones).
 *
 * pair-setup saves each controller's long-term Ed25519 public key keyed by its
 * identifier; pair-verify looks it up to authenticate a reconnecting phone.
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'

function dir(): string {
  return join(app.getPath('userData'), 'cp')
}
function file(): string {
  return join(dir(), 'pairings.json')
}

function load(): Record<string, string> {
  try {
    if (existsSync(file()))
      return JSON.parse(readFileSync(file(), 'utf8')) as Record<string, string>
  } catch {
    /* start fresh */
  }
  return {}
}

export function savePairing(identifier: string, ltpk: Buffer): void {
  const p = load()
  p[identifier] = ltpk.toString('hex')
  try {
    mkdirSync(dir(), { recursive: true })
    writeFileSync(file(), JSON.stringify(p), { mode: 0o600 })
  } catch (e) {
    console.warn('[cpPairings] persist failed:', (e as Error).message)
  }
}

export function getPairing(identifier: string): Buffer | null {
  const hex = load()[identifier]
  return hex ? Buffer.from(hex, 'hex') : null
}
