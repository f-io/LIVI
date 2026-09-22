import { execFileSync, spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { type BrowserWindow, dialog } from 'electron'
import {
  helperInstalls,
  markerHolds,
  pkexecAvailable,
  runAsRoot,
  sudoersLines,
  sudoGrants,
  username,
  writeMarker
} from './privileged'

const GUARD_DIR = '/usr/local/lib/livi'
const GUARD_PATH = `${GUARD_DIR}/gvfs-phone-guard.sh`
const SUDOERS_FILE = '/etc/sudoers.d/99-LIVI-gvfs'
const MARKER = 'gvfs-guard-v1.installed'

const MONITOR_DIR = '/usr/share/gvfs/remote-volume-monitors'
const PHONE_MONITORS = ['afc', 'gphoto2', 'mtp']

const GUARD_SCRIPT = `#!/bin/bash
set -u
D=${MONITOR_DIR}
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

function phoneMonitorsPresent(): boolean {
  return PHONE_MONITORS.some(
    (m) => existsSync(`${MONITOR_DIR}/${m}.monitor`) || existsSync(`${MONITOR_DIR}/${m}.livi-off`)
  )
}

function ruleContent(): string {
  const user = username()
  return [
    `# Installed by LIVI — lets ${user} toggle the phone gvfs volume monitors while LIVI`,
    `# runs, and restore them when it exits. Remove this file to revoke.`,
    `Cmnd_Alias LIVI_GVFS = ${GUARD_PATH} disable, ${GUARD_PATH} restore`,
    `${user} ALL=(root) NOPASSWD: LIVI_GVFS`,
    ''
  ].join('\n')
}

function isInstalled(): boolean {
  return existsSync(GUARD_PATH) && (sudoGrants(GUARD_PATH) || markerHolds(MARKER, ruleContent()))
}

/** One-time: install the privileged toggle script + sudoers so LIVI can hide plugged
 *  phones from the desktop file manager while it runs. No-op once installed. */
export async function checkAndInstallGvfsGuard(window: BrowserWindow): Promise<void> {
  if (process.platform !== 'linux') return
  if (isInstalled()) return
  if (process.env.LIVI_KIOSK === '1') return
  if (!phoneMonitorsPresent()) return

  const content = ruleContent()
  if (helperInstalls('install-gvfs-guard', { script: GUARD_SCRIPT, rule: content })) {
    try {
      writeMarker(MARKER, content)
    } catch {}
    return
  }

  if (!pkexecAvailable()) {
    console.warn('[gvfsGuard] pkexec not available — cannot install phone-guard')
    return
  }

  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: 'LIVI',
    message: 'Hide connected phones from the file manager?',
    buttons: ['Install', 'Skip'],
    defaultId: 0,
    cancelId: 1
  })
  if (response !== 0) return

  try {
    await runAsRoot([
      `mkdir -p ${GUARD_DIR}`,
      `cat > ${GUARD_PATH} <<'GUARDEOF'`,
      GUARD_SCRIPT.trimEnd(),
      'GUARDEOF',
      `chmod 0755 ${GUARD_PATH}`,
      `chown root:root ${GUARD_PATH}`,
      ...sudoersLines(SUDOERS_FILE, content)
    ])
    try {
      writeMarker(MARKER, content)
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
