import { execFileSync, spawn } from 'node:child_process'
import { existsSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'
import { app, type BrowserWindow, dialog } from 'electron'

const GUARD_DIR = '/usr/local/lib/livi'
const GUARD_PATH = `${GUARD_DIR}/gvfs-phone-guard.sh`
const SUDOERS_FILE = '/etc/sudoers.d/99-LIVI-gvfs'
const SENTINEL_VERSION = 'v1'

const GUARD_SCRIPT = `#!/bin/bash
set -u
D=/usr/share/gvfs/remote-volume-monitors
action="\${1:-}"
for m in afc gphoto2 mtp; do
  case "$action" in
    disable) [ -f "$D/$m.monitor" ] && mv "$D/$m.monitor" "$D/$m.livi-off" ;;
    restore) [ -f "$D/$m.livi-off" ] && mv "$D/$m.livi-off" "$D/$m.monitor" ;;
    *) echo "usage: gvfs-phone-guard.sh disable|restore" >&2 ; exit 2 ;;
  esac
done
[ "$action" = disable ] && pkill -f "gvfs-afc-volume|gvfs-gphoto2|gvfs-mtp-volume|gvfsd-afc" 2>/dev/null
exit 0
`

function sentinelPath(): string {
  return join(app.getPath('userData'), `gvfs-guard-${SENTINEL_VERSION}.installed`)
}

function resolveUsername(): string {
  if (process.env.SUDO_USER) return process.env.SUDO_USER
  return os.userInfo().username
}

function ruleActiveInSudo(): boolean {
  try {
    const out = execFileSync('sudo', ['-n', '-l'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore']
    })
    return out.includes('LIVI_GVFS') || out.includes(GUARD_PATH)
  } catch {
    return false
  }
}

function isInstalled(): boolean {
  return existsSync(GUARD_PATH) && (ruleActiveInSudo() || existsSync(sentinelPath()))
}

function pkexecAvailable(): boolean {
  try {
    execFileSync('which', ['pkexec'], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

function buildSudoers(): string {
  const user = resolveUsername()
  return [
    `# Installed by LIVI — lets ${user} toggle the phone gvfs volume monitors while LIVI`,
    `# runs, and restore them when it exits. Remove this file to revoke.`,
    `Cmnd_Alias LIVI_GVFS = ${GUARD_PATH} disable, ${GUARD_PATH} restore`,
    `${user} ALL=(root) NOPASSWD: LIVI_GVFS`,
    ''
  ].join('\n')
}

function installViaPkexec(): Promise<void> {
  const tmpSudo = `${SUDOERS_FILE}.livi-tmp`
  const script = [
    `mkdir -p ${GUARD_DIR}`,
    `cat > ${GUARD_PATH} <<'GUARDEOF'`,
    GUARD_SCRIPT.trimEnd(),
    'GUARDEOF',
    `chmod 0755 ${GUARD_PATH}`,
    `chown root:root ${GUARD_PATH}`,
    `cat > ${tmpSudo} <<'SUDOEOF'`,
    buildSudoers().trimEnd(),
    'SUDOEOF',
    `chmod 0440 ${tmpSudo}`,
    `chown root:root ${tmpSudo}`,
    `visudo -c -f ${tmpSudo}`,
    `mv ${tmpSudo} ${SUDOERS_FILE}`
  ].join('\n')

  return new Promise((resolve, reject) => {
    const proc = spawn('pkexec', ['bash', '-c', script], { stdio: 'ignore' })
    proc.on('close', (code) =>
      code === 0 ? resolve() : reject(new Error(`pkexec exited ${code}`))
    )
    proc.on('error', reject)
  })
}

/** One-time: install the privileged toggle script + sudoers so LIVI can hide plugged
 *  phones from the desktop file manager while it runs. No-op once installed. */
export async function checkAndInstallGvfsGuard(window: BrowserWindow): Promise<void> {
  if (process.platform !== 'linux') return
  if (isInstalled()) return
  if (!pkexecAvailable()) {
    console.warn('[gvfsGuard] pkexec not available — cannot install phone-guard')
    return
  }

  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: 'LIVI — Permission Required',
    message: 'Allow LIVI to hide a plugged-in phone from the desktop file manager while running?',
    detail:
      `A small helper (${GUARD_PATH}) and a sudoers rule (${SUDOERS_FILE}) will be installed. ` +
      `The phone volume monitors are only disabled while LIVI runs and are restored when it ` +
      `exits, so the phone stays accessible on the desktop without LIVI.`,
    buttons: ['Install', 'Skip'],
    defaultId: 0,
    cancelId: 1
  })
  if (response !== 0) return

  try {
    await installViaPkexec()
    try {
      writeFileSync(sentinelPath(), `${new Date().toISOString()} ${GUARD_PATH}\n`, { mode: 0o644 })
    } catch {}
  } catch (err) {
    console.error('[gvfsGuard] install failed:', err)
  }
}

/** Disable the phone gvfs volume monitors for the lifetime of this LIVI process.
 *  Heals any leftover from a prior crash first, then spawns a detached watcher that
 *  restores them when this process dies, however it dies (SIGKILL/crash included). */
export function startPhoneSuppression(): void {
  if (process.platform !== 'linux' || !existsSync(GUARD_PATH)) return
  try {
    execFileSync('sudo', ['-n', GUARD_PATH, 'restore'], { stdio: 'ignore' })
    execFileSync('sudo', ['-n', GUARD_PATH, 'disable'], { stdio: 'ignore' })
  } catch (e) {
    console.warn('[gvfsGuard] could not disable phone monitors:', (e as Error).message)
    return
  }
  const cmd = `while kill -0 ${process.pid} 2>/dev/null; do sleep 2; done; sudo -n ${GUARD_PATH} restore`
  const guard = spawn('bash', ['-c', cmd], { detached: true, stdio: 'ignore' })
  guard.unref()
}

/** Restore the phone gvfs volume monitors (on a clean LIVI exit). Idempotent; the
 *  detached watcher would restore anyway if the process is killed before this runs. */
export function stopPhoneSuppression(): void {
  if (process.platform !== 'linux' || !existsSync(GUARD_PATH)) return
  try {
    execFileSync('sudo', ['-n', GUARD_PATH, 'restore'], { stdio: 'ignore' })
  } catch (e) {
    console.warn('[gvfsGuard] could not restore phone monitors:', (e as Error).message)
  }
}
