import { execFile } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'

const DROPIN_DIR = join(os.homedir(), '.config', 'wireplumber', 'wireplumber.conf.d')
const DROPIN_PATH = join(DROPIN_DIR, '99-livi-hfp.conf')

// The helper owns HFP (SLC + SCO). WirePlumber keeps A2DP for BT speakers and
// must not register the HFP/HSP profiles, or the helper's registration fails.
const DROPIN_CONTENT = `monitor.bluez.properties = {
  bluez5.roles = [ a2dp_sink a2dp_source ]
}
`

/** Installs the WirePlumber role restriction; restarts wireplumber only on change. */
export function ensureWireplumberBtRoles(): void {
  if (process.platform !== 'linux') return
  try {
    const current = existsSync(DROPIN_PATH) ? readFileSync(DROPIN_PATH, 'utf8') : ''
    if (current === DROPIN_CONTENT) return
    mkdirSync(DROPIN_DIR, { recursive: true })
    writeFileSync(DROPIN_PATH, DROPIN_CONTENT)
    console.log(`[wireplumberBtRoles] installed ${DROPIN_PATH}, restarting wireplumber`)
    execFile('systemctl', ['--user', 'restart', 'wireplumber'], (err) => {
      if (err) console.warn(`[wireplumberBtRoles] restart failed: ${err.message}`)
    })
  } catch (e) {
    console.warn(`[wireplumberBtRoles] install failed: ${(e as Error).message}`)
  }
}
