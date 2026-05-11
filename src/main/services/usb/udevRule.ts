import { execFileSync, spawn } from 'child_process'
import { BrowserWindow, dialog } from 'electron'
import fs from 'fs'
import os from 'os'

const RULE_FILE = '/etc/udev/rules.d/99-LIVI.rules'

// Bumped whenever buildRuleContent's output changes — forces existing
// installs to re-prompt for an upgrade.
const RULE_VERSION = 3
const RULE_VERSION_MARKER = `# LIVI-RULE-VERSION=${RULE_VERSION}`

const IGNORE_VARS =
  'ENV{ID_MTP_DEVICE}="", ENV{ID_MEDIA_PLAYER}="", ENV{UDISKS_IGNORE}="1", ENV{ID_MM_DEVICE_IGNORE}="1"'

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

  // Carlinkit dongle + AOAP accessory mode + common Android phone vendors.
  const phoneVendors = [
    '18d1', // Google / generic Android
    '04e8', // Samsung
    '22b8', // Motorola
    '0fce', // Sony
    '12d1', // Huawei
    '2717', // Xiaomi
    '2a70', // OnePlus
    '0bb4', // HTC / Sharp
    '1004', // LG
    '19d2', // ZTE
    '109b', // Hisense
    '0e8d', // MediaTek (reference phones)
    '17ef', // Lenovo / Motorola
    '04dd', // Sharp
    '0489', // Foxconn / OEM Asus
    '0b05' // Asus
  ]

  const lines: string[] = [
    RULE_VERSION_MARKER,
    `SUBSYSTEM=="usb", ATTR{idVendor}=="1314", ATTR{idProduct}=="152*", MODE="0660", OWNER="${username}"`,
    `SUBSYSTEM=="usb", ATTR{idVendor}=="18d1", ATTR{idProduct}=="2d0[0-5]", MODE="0660", OWNER="${username}", ${IGNORE_VARS}`,
    ...phoneVendors.map(
      (vid) =>
        `SUBSYSTEM=="usb", ATTR{idVendor}=="${vid}", MODE="0660", OWNER="${username}", ${IGNORE_VARS}`
    )
  ]

  return lines.join('\n') + '\n'
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
    return content.includes(RULE_VERSION_MARKER)
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
