import { execFileSync, spawn } from 'child_process'
import { BrowserWindow, dialog } from 'electron'
import fs from 'fs'
import os from 'os'

const RULE_FILE = '/etc/udev/rules.d/99-LIVI.rules'

function buildRuleContent(): string {
  let username = ''

  if (process.env.PKEXEC_UID) {
    try {
      username = execFileSync('id', ['-nu', process.env.PKEXEC_UID], {
        encoding: 'utf8'
      }).trim()
    } catch {
      // fall through
    }
  }

  if (!username && process.env.SUDO_USER) {
    username = process.env.SUDO_USER
  }

  if (!username) {
    username = os.userInfo().username
  }

  return `SUBSYSTEM=="usb", ATTR{idVendor}=="1314", ATTR{idProduct}=="152*", MODE="0660", OWNER="${username}"\n`
}

export function udevRuleExists(): boolean {
  try {
    return fs.existsSync(RULE_FILE)
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
  if (udevRuleExists()) return
  if (!pkexecAvailable()) {
    console.warn('[udevRule] pkexec not available, skipping udev rule setup')
    return
  }

  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: 'USB Permission Required',
    message: 'LIVI needs permission to access the USB dongle.',
    detail: 'A udev rule will be installed to /etc/udev/rules.d/99-LIVI.rules.',
    buttons: ['Install', 'Skip'],
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
      detail: `Run this manually:\n\nsudo bash -c 'echo "${ruleContent.trim()}" > ${RULE_FILE} && udevadm control --reload-rules && udevadm trigger'`,
      buttons: ['OK']
    })
  }
}
