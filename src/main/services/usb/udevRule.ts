import { execFileSync, spawn } from 'child_process'
import { BrowserWindow, dialog } from 'electron'
import fs from 'fs'
import os from 'os'
import path from 'path'

const RULE_FILE = '/etc/udev/rules.d/99-LIVI.rules'

const TEMPLATE_FILENAME = '99-LIVI.rules.template'

function resolveTemplatePath(): string {
  const resources = process.resourcesPath
  if (typeof resources === 'string' && resources.length > 0) {
    const packaged = path.join(resources, TEMPLATE_FILENAME)
    if (fs.existsSync(packaged)) return packaged
  }
  return path.join(__dirname, '..', '..', '..', '..', 'assets', 'linux', TEMPLATE_FILENAME)
}

function loadTemplate(): string {
  return fs.readFileSync(resolveTemplatePath(), 'utf8')
}

let cachedPhoneVendorIds: Set<number> | null | undefined

// Android vendor allowlist parsed from the udev template.
// Lines that also match an idProduct (dongle) are skipped.
export function phoneVendorIdsFromUdevTemplate(): Set<number> | null {
  if (cachedPhoneVendorIds !== undefined) return cachedPhoneVendorIds
  try {
    const ids = new Set<number>()
    for (const line of loadTemplate().split('\n')) {
      if (line.includes('ATTR{idProduct}')) continue
      const m = line.match(/ATTR\{idVendor\}=="([0-9a-fA-F]+)"/)
      if (m) ids.add(Number.parseInt(m[1], 16))
    }
    cachedPhoneVendorIds = ids.size > 0 ? ids : null
  } catch {
    cachedPhoneVendorIds = null
  }
  return cachedPhoneVendorIds
}

function templateMarker(template: string): string {
  const m = template.match(/^# LIVI-RULE-VERSION=\d+$/m)
  return m ? m[0] : '# LIVI-RULE-VERSION=0'
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
  return loadTemplate().replace(/__USERNAME__/g, resolveUsername())
}

export function udevRuleExists(): boolean {
  try {
    return fs.existsSync(RULE_FILE)
  } catch {
    return false
  }
}

function udevRuleIsCurrent(): boolean {
  try {
    if (!fs.existsSync(RULE_FILE)) return false
    const content = fs.readFileSync(RULE_FILE, 'utf8')
    return content.includes(templateMarker(loadTemplate()))
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
    const script =
      'set -e\n' +
      `cat > ${RULE_FILE} <<'LIVI_RULE_EOF'\n${buildRuleContent().trim()}\nLIVI_RULE_EOF\n` +
      'udevadm control --reload-rules\n' +
      'udevadm trigger'

    const proc = spawn('pkexec', ['bash', '-c', script], { stdio: 'ignore' })

    proc.on('close', (code) => {
      if (code === 0) {
        resolve()
      } else {
        reject(new Error(`pkexec exited with code ${code}`))
      }
    })

    proc.on('error', reject)
  })
}

export async function checkAndInstallUdevRule(window: BrowserWindow): Promise<boolean> {
  if (process.platform !== 'linux') return false

  const exists = udevRuleExists()
  const isCurrent = exists && udevRuleIsCurrent()
  if (exists && isCurrent) return false

  if (!pkexecAvailable()) {
    console.warn('[udevRule] pkexec not available, skipping udev rule setup')
    return false
  }

  const isUpgrade = exists && !isCurrent
  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: isUpgrade ? 'USB Permission Update' : 'USB Permission Required',
    message: isUpgrade
      ? 'LIVI needs to update its udev rule for USB device access.'
      : 'LIVI needs permission to access USB devices.',
    detail: isUpgrade
      ? `The existing rule at ${RULE_FILE} is outdated (wired Android Auto needs additional phone vendor entries). It will be replaced.`
      : `A udev rule will be installed to ${RULE_FILE}.`,
    buttons: [isUpgrade ? 'Update' : 'Install', 'Skip'],
    defaultId: 0,
    cancelId: 1
  })

  if (response !== 0) return false

  let installed = false
  while (!installed) {
    try {
      await installRule()
      installed = true
    } catch (err) {
      console.error('[udevRule] Installation failed:', err)
      const { response: retry } = await dialog.showMessageBox(window, {
        type: 'error',
        title: 'Installation Failed',
        message: 'Could not install the udev rule.',
        detail: `${err instanceof Error ? err.message : String(err)}\n\nThis step is required for USB device access.`,
        buttons: ['Retry', 'Skip'],
        defaultId: 0,
        cancelId: 1
      })
      if (retry !== 0) return false
    }
  }

  await dialog.showMessageBox(window, {
    type: 'info',
    title: 'Done',
    message: 'udev rule installed. LIVI will now restart to apply it.',
    buttons: ['OK']
  })
  return true
}
