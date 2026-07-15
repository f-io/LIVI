import { execFileSync, spawn } from 'node:child_process'
import { existsSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'
import { app, BrowserWindow, dialog } from 'electron'

const RULE_FILE = '/etc/sudoers.d/99-LIVI-bt'
const SENTINEL_VERSION = 'v1'
function sentinelPath(): string {
  return join(app.getPath('userData'), `bt-sudoers-${SENTINEL_VERSION}.installed`)
}

function pythonPath(): string {
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
    } catch {}
  }
  if (process.env.SUDO_USER) return process.env.SUDO_USER
  return os.userInfo().username
}

function buildRuleContent(): string {
  const user = resolveUsername()
  const py = pythonPath()
  return [
    `# Installed by LIVI — allows ${user} to run livi-helper.py as root`,
    `# without a password prompt. Remove this file to revoke.`,
    `Cmnd_Alias LIVI_BT = ${py} *livi-helper.py`,
    `${user} ALL=(root) NOPASSWD: SETENV: LIVI_BT`,
    ''
  ].join('\n')
}

function ruleActiveInSudo(): boolean {
  try {
    const out = execFileSync('sudo', ['-n', '-l'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore']
    })
    return out.includes('LIVI_BT') || out.includes('livi-helper.py')
  } catch {
    return false
  }
}

export function helperSudoersExists(): boolean {
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

export async function checkAndInstallHelperSudoers(window: BrowserWindow): Promise<void> {
  if (process.platform !== 'linux') return
  if (helperSudoersExists()) return
  if (!pkexecAvailable()) {
    console.warn('[helperSudoers] pkexec not available — cannot install sudoers drop-in')
    return
  }

  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: 'Wireless Projection — Permission Required',
    message:
      'LIVI needs permission to manage Bluetooth and Wi-Fi for wireless Android Auto / CarPlay.',
    detail:
      `A sudoers rule will be installed at ${RULE_FILE} so the BT/Wi-Fi helper ` +
      `(livi-helper.py) can run as root without prompting on each session.`,
    buttons: ['Install', 'Skip'],
    defaultId: 0,
    cancelId: 1
  })
  if (response !== 0) return

  try {
    await installRule()
    try {
      writeFileSync(sentinelPath(), `${new Date().toISOString()} ${RULE_FILE}\n`, { mode: 0o644 })
    } catch (e) {
      console.warn('[helperSudoers] could not write sentinel:', (e as Error).message)
    }
    await dialog.showMessageBox(window, {
      type: 'info',
      title: 'Done',
      message: 'Wireless projection permissions installed.',
      buttons: ['OK']
    })
  } catch (err) {
    console.error('[helperSudoers] installation failed:', err)
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
