/**
 * aaSudoers — first-run installer for the sudoers drop-in that lets LIVI run
 * `aa-bluetooth.py` as root without a password prompt at runtime.
 *
 */
import { execFileSync, spawn } from 'node:child_process'
import { existsSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'
import { app, BrowserWindow, dialog } from 'electron'

const RULE_FILE = '/etc/sudoers.d/99-LIVI-aa'

/**
 * Per-user sentinel marking that we have successfully installed the sudoers
 * drop-in. We can't probe `/etc/sudoers.d/` directly: on Debian/Pi OS that
 * directory is mode 0750 root:root
 * The sentinel lives in userData (always traversable) and is bumped via
 * a version suffix when the rule format changes.
 */
const SENTINEL_VERSION = 'v2'
function sentinelPath(): string {
  return join(app.getPath('userData'), `aa-sudoers-${SENTINEL_VERSION}.installed`)
}

function pythonPath(): string {
  // sudoers Cmnd_Spec needs an absolute path; PATH-relative names are rejected.
  for (const p of ['/usr/bin/python3', '/usr/local/bin/python3']) {
    if (existsSync(p)) return p
  }
  try {
    return execFileSync('which', ['python3'], { encoding: 'utf8' }).trim() || '/usr/bin/python3'
  } catch {
    return '/usr/bin/python3'
  }
}

function resolveUsername(): string {
  if (process.env.PKEXEC_UID) {
    try {
      return execFileSync('id', ['-nu', process.env.PKEXEC_UID], { encoding: 'utf8' }).trim()
    } catch {
      // fall through
    }
  }
  if (process.env.SUDO_USER) return process.env.SUDO_USER
  return os.userInfo().username
}

function buildRuleContent(): string {
  const user = resolveUsername()
  const py = pythonPath()
  // SETENV: lets the supervisor pass LIVI_SSID / LIVI_PASSPHRASE / … through
  // sudo without an explicit env_keep list. NOEXEC: blocks the script from
  // re-execing other privileged binaries via system() — defence-in-depth.
  return [
    `# Installed by LIVI — allows ${user} to run aa-bluetooth.py as root`,
    `# without a password prompt. Remove this file to revoke.`,
    `Cmnd_Alias LIVI_AA_BT = ${py} *aa-bluetooth.py`,
    `${user} ALL=(root) NOPASSWD: SETENV: LIVI_AA_BT`,
    ''
  ].join('\n')
}

/**
 * Ground-truth probe: ask sudo itself whether the LIVI alias is allowed
 * non-interactively. `sudo -n -l` lists the user's permitted commands without
 * prompting — if the rule file is in place and SETENV: NOPASSWD is honoured,
 * the LIVI_AA_BT alias appears in the listing. If sudo would need a password
 * for `-l`, it exits non-zero; we catch and fall through to the sentinel.
 */
function ruleActiveInSudo(): boolean {
  try {
    const out = execFileSync('sudo', ['-n', '-l'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore']
    })
    return out.includes('LIVI_AA_BT') || out.includes('aa-bluetooth.py')
  } catch {
    return false
  }
}

export function aaSudoersExists(): boolean {
  if (ruleActiveInSudo()) return true
  try {
    return existsSync(sentinelPath())
  } catch {
    return false
  }
}

function pkexecAvailable(): boolean {
  try {
    execFileSync('which', ['pkexec'], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

function installRule(): Promise<void> {
  return new Promise((resolve, reject) => {
    const content = buildRuleContent()
    // Heredoc avoids quoting hell with embedded newlines. visudo -c -f sanity
    // checks the file before we move it into place; on parse error we delete
    // the temp instead of leaving a broken sudoers entry.
    const tmpFile = `${RULE_FILE}.livi-tmp`
    const script = [
      `cat > ${tmpFile} <<'EOF'`,
      content.trimEnd(),
      'EOF',
      `chmod 0440 ${tmpFile}`,
      `chown root:root ${tmpFile}`,
      `visudo -c -f ${tmpFile}`,
      `mv ${tmpFile} ${RULE_FILE}`
    ].join('\n')

    const proc = spawn('pkexec', ['bash', '-c', script], { stdio: 'ignore' })
    proc.on('close', (code) => {
      if (code === 0) resolve()
      else reject(new Error(`pkexec exited with code ${code}`))
    })
    proc.on('error', reject)
  })
}

export async function checkAndInstallAaSudoers(window: BrowserWindow): Promise<void> {
  if (process.platform !== 'linux') return
  if (aaSudoersExists()) return
  if (!pkexecAvailable()) {
    console.warn('[aaSudoers] pkexec not available — cannot install sudoers drop-in')
    return
  }

  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: 'Wireless Android Auto — Permission Required',
    message: 'LIVI needs permission to manage Bluetooth and Wi-Fi for wireless Android Auto.',
    detail:
      `A sudoers rule will be installed at ${RULE_FILE} so the BT/Wi-Fi helper ` +
      `(aa-bluetooth.py) can run as root without prompting on each session.`,
    buttons: ['Install', 'Skip'],
    defaultId: 0,
    cancelId: 1
  })
  if (response !== 0) return

  try {
    await installRule()
    // Drop the per-user sentinel so subsequent launches skip the prompt even
    // though we can't stat /etc/sudoers.d/. ruleActiveInSudo() is still the
    // ground truth — the sentinel just avoids re-prompting the user when the
    // sudoers dir is unreadable to us.
    try {
      writeFileSync(sentinelPath(), `${new Date().toISOString()} ${RULE_FILE}\n`, { mode: 0o644 })
    } catch (e) {
      console.warn('[aaSudoers] could not write sentinel:', (e as Error).message)
    }
    await dialog.showMessageBox(window, {
      type: 'info',
      title: 'Done',
      message: 'Wireless AA permissions installed.',
      buttons: ['OK']
    })
  } catch (err) {
    console.error('[aaSudoers] installation failed:', err)
    const content = buildRuleContent()
    await dialog.showMessageBox(window, {
      type: 'error',
      title: 'Installation Failed',
      message: 'Could not install the sudoers drop-in.',
      detail:
        `Run this manually:\n\nsudo tee ${RULE_FILE} <<'EOF'\n${content.trimEnd()}\nEOF\n` +
        `sudo chmod 0440 ${RULE_FILE}`,
      buttons: ['OK']
    })
  }
}
