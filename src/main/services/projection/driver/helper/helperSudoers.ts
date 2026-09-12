import {
  asset,
  markerHolds,
  pkexecAvailable,
  runAsRoot,
  sudoersLines,
  sudoGrants,
  username,
  writeMarker
} from '@main/services/privileged'
import { BrowserWindow, dialog } from 'electron'

const RULE_FILE = '/etc/sudoers.d/99-LIVI-helper'
// The rule covers the whole helper, not only Bluetooth. This name replaces 99-LIVI-bt.
const OBSOLETE_RULE_FILE = '/etc/sudoers.d/99-LIVI-bt'
const TEMPLATE = '99-LIVI-helper.sudoers.template'
const MARKER = 'helper-sudoers-v1.installed'

function ruleContent(): string {
  return asset(TEMPLATE).replace(/__USERNAME__/g, username())
}

export function helperSudoersExists(): boolean {
  // Only a rule naming the helper binary counts; an older python-era rule does not.
  return sudoGrants('livi-helperd') || markerHolds(MARKER, ruleContent())
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
      `(livi-helperd) can run as root without prompting on each session.`,
    buttons: ['Install', 'Skip'],
    defaultId: 0,
    cancelId: 1
  })
  if (response !== 0) return

  const content = ruleContent()
  try {
    await runAsRoot([...sudoersLines(RULE_FILE, content), `rm -f ${OBSOLETE_RULE_FILE}`])
    try {
      writeMarker(MARKER, content)
    } catch (e) {
      console.warn('[helperSudoers] could not write marker:', (e as Error).message)
    }
    await dialog.showMessageBox(window, {
      type: 'info',
      title: 'Done',
      message: 'Wireless projection permissions installed.',
      buttons: ['OK']
    })
  } catch (err) {
    console.error('[helperSudoers] installation failed:', err)
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
