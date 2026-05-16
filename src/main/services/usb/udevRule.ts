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
    const ruleContent = buildRuleContent()
    const script = [
      `echo '${ruleContent.trim()}' > ${RULE_FILE}`,
      'udevadm control --reload-rules',
      'udevadm trigger'
    ].join(' && ')

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

export async function checkAndInstallUdevRule(window: BrowserWindow): Promise<void> {
  if (process.platform !== 'linux') return

  const exists = udevRuleExists()
  const isCurrent = exists && udevRuleIsCurrent()
  if (exists && isCurrent) return

  if (!pkexecAvailable()) {
    console.warn('[udevRule] pkexec not available, skipping udev rule setup')
    return
  }

  const isUpgrade = exists && !isCurrent
  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: isUpgrade ? 'USB Permission Update' : 'USB Permission Required',
    message: isUpgrade
      ? 'LIVI needs to update its udev rule for USB device access.'
      : 'LIVI needs permission to access the USB dongle.',
    detail: isUpgrade
      ? `The existing rule at ${RULE_FILE} is outdated (wired Android Auto needs additional phone vendor entries). It will be replaced.`
      : `A udev rule will be installed to ${RULE_FILE}.`,
    buttons: [isUpgrade ? 'Update' : 'Install', 'Skip'],
    defaultId: 0,
    cancelId: 1
  })

  if (response !== 0) return

  try {
    await installRule()
    await dialog.showMessageBox(window, {
      type: 'info',
      title: 'Done',
      message: 'udev rule installed successfully.',
      buttons: ['OK']
    })
  } catch (err) {
    console.error('[udevRule] Installation failed:', err)
    const ruleContent = buildRuleContent()
    await dialog.showMessageBox(window, {
      type: 'error',
      title: 'Installation Failed',
      message: 'Could not install the udev rule.',
      detail: `Run this manually:\n\nsudo tee ${RULE_FILE} <<'EOF'\n${ruleContent.trim()}\nEOF\nsudo udevadm control --reload-rules && sudo udevadm trigger`,
      buttons: ['OK']
    })
  }
}
