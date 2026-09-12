import { existsSync, readFileSync } from 'node:fs'
import { dirname } from 'node:path'
import { BrowserWindow, dialog } from 'electron'
import { asset, helperInstalls, pkexecAvailable, runAsRoot } from '../privileged'

const RULE_FILE = '/etc/udev/rules.d/99-LIVI.rules'
const TEMPLATE = '99-LIVI.rules.template'

const TOUCH_FILTER = 'livi-touch-filter'
const TOUCH_FILTER_FILE = '/usr/local/lib/livi/livi-touch-filter'

// The rule calls this to tell a real mouse from a touch panel's mouse interface.
function loadTouchFilter(): string | null {
  try {
    return asset(TOUCH_FILTER)
  } catch {
    return null
  }
}

function templateMarker(template: string): string {
  const m = template.match(/^# LIVI-RULE-VERSION=\d+$/m)
  return m ? m[0] : '# LIVI-RULE-VERSION=0'
}

export function udevRuleExists(): boolean {
  try {
    return existsSync(RULE_FILE)
  } catch {
    return false
  }
}

function udevRuleIsCurrent(): boolean {
  try {
    if (!existsSync(RULE_FILE)) return false
    const content = readFileSync(RULE_FILE, 'utf8')
    return content.includes(templateMarker(asset(TEMPLATE)))
  } catch {
    return false
  }
}

function installRule(): Promise<void> {
  const filter = loadTouchFilter()
  return runAsRoot([
    `cat > ${RULE_FILE} <<'LIVI_RULE_EOF'`,
    asset(TEMPLATE).trim(),
    'LIVI_RULE_EOF',
    ...(filter
      ? [
          `mkdir -p ${dirname(TOUCH_FILTER_FILE)}`,
          `cat > ${TOUCH_FILTER_FILE} <<'LIVI_FILTER_EOF'`,
          filter.trim(),
          'LIVI_FILTER_EOF',
          `chmod 0755 ${TOUCH_FILTER_FILE}`
        ]
      : []),
    'udevadm control --reload-rules',
    'udevadm trigger'
  ])
}

export async function checkAndInstallUdevRule(window: BrowserWindow): Promise<boolean> {
  if (process.platform !== 'linux') return false

  const exists = udevRuleExists()
  const isCurrent = exists && udevRuleIsCurrent()
  if (exists && isCurrent) return false

  const filter = loadTouchFilter()
  if (
    helperInstalls(
      'install-udev-rule',
      filter ? { rule: asset(TEMPLATE), filter } : { rule: asset(TEMPLATE) }
    )
  ) {
    return true
  }

  if (!pkexecAvailable()) {
    console.warn('[udevRule] pkexec not available, skipping udev rule setup')
    return false
  }

  const isUpgrade = exists && !isCurrent
  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: isUpgrade ? 'udev Rule Update' : 'udev Rule Required',
    message: isUpgrade
      ? 'LIVI needs to update its udev rule.'
      : 'LIVI needs a udev rule for its touch panels and phones.',
    detail: isUpgrade
      ? `The existing rule at ${RULE_FILE} is outdated. It will be replaced.`
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
        detail: `${err instanceof Error ? err.message : String(err)}`,
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
