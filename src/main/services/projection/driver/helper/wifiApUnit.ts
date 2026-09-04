import { execFileSync, spawn } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'
import type { Config } from '@shared/types/Config'
import { app, type BrowserWindow, dialog } from 'electron'

const UNIT_PATH = '/etc/systemd/system/livi-wifi-ap.service'
const SUDOERS_PATH = '/etc/sudoers.d/99-LIVI-wifi-ap'
const SERVICE = 'livi-wifi-ap.service'
const NM_UNMANAGED_CONF = '/etc/NetworkManager/conf.d/99-livi-ap-unmanaged.conf'
// Bump when the unit or sudoers content changes.
const INSTALL_VERSION = '3'

function helperPath(): string {
  return join(app.getPath('userData'), 'driver', 'livi-helperd')
}

function markerPath(): string {
  return join(app.getPath('userData'), '.wifi-ap-install')
}

function systemctlPath(): string {
  try {
    return execFileSync('which', ['systemctl'], { encoding: 'utf8' }).trim() || '/usr/bin/systemctl'
  } catch {
    return '/usr/bin/systemctl'
  }
}

function unitContent(): string {
  const helper = helperPath()
  const user = os.userInfo().username
  return `[Unit]
Description=LIVI wireless projection AP (early boot)
After=network-pre.target
Wants=network-pre.target
ConditionPathExists=${helper}

[Service]
Type=simple
Environment=SUDO_USER=${user}
ExecStart=${helper} --wifi-ap
ExecStop=${helper} --wifi-ap-teardown
TimeoutStopSec=8
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
`
}

// NOPASSWD so LIVI can manage the AP service and teardown without a prompt.
function sudoersContent(): string {
  const user = os.userInfo().username
  const sc = systemctlPath()
  const helper = helperPath()
  return `Cmnd_Alias LIVI_WIFI_AP = ${sc} start ${SERVICE}, ${sc} stop ${SERVICE}, ${sc} enable ${SERVICE}, ${sc} disable ${SERVICE}, ${helper} --wifi-ap-teardown
${user} ALL=(root) NOPASSWD: LIVI_WIFI_AP
`
}

function readFile(path: string): string {
  try {
    return existsSync(path) ? readFileSync(path, 'utf8') : ''
  } catch {
    return ''
  }
}

// The sudoers file is root-only, so a marker tracks its version.
function needsInstall(): boolean {
  return readFile(UNIT_PATH) !== unitContent() || readFile(markerPath()).trim() !== INSTALL_VERSION
}

function installPrivileged(): Promise<void> {
  const script = [
    'set -e',
    `cat > ${UNIT_PATH} <<'EOF'`,
    unitContent().trimEnd(),
    'EOF',
    `cat > ${SUDOERS_PATH} <<'EOF'`,
    sudoersContent().trimEnd(),
    'EOF',
    `chmod 440 ${SUDOERS_PATH}`,
    'systemctl daemon-reload'
  ].join('\n')
  return new Promise((resolve, reject) => {
    const proc = spawn('pkexec', ['bash', '-c', script], { stdio: 'ignore' })
    proc.on('close', (code) =>
      code === 0 ? resolve() : reject(new Error(`pkexec exited ${code}`))
    )
    proc.on('error', reject)
  })
}

function sudo(args: string[]): Promise<boolean> {
  return new Promise((resolve) => {
    const proc = spawn('sudo', ['-n', ...args], { stdio: 'ignore' })
    proc.on('close', (code) => resolve(code === 0))
    proc.on('error', () => resolve(false))
  })
}

function apWanted(config: Config): boolean {
  return config.wifiDedicatedInterface || config.wirelessCpEnabled || config.wirelessAaEnabled
}

function cmdOk(cmd: string, args: string[]): Promise<boolean> {
  return new Promise((resolve) => {
    const proc = spawn(cmd, args, { stdio: 'ignore' })
    proc.on('close', (code) => resolve(code === 0))
    proc.on('error', () => resolve(false))
  })
}

// Undo a takeover and return the interface to NetworkManager.
async function releaseInterface(config: Config): Promise<void> {
  const taken =
    existsSync(NM_UNMANAGED_CONF) ||
    (await cmdOk('systemctl', ['is-active', '--quiet', SERVICE])) ||
    (await cmdOk('systemctl', ['is-enabled', '--quiet', SERVICE]))
  if (!taken) return
  const iface = config.wifiInterface || 'wlan0'
  const script = [
    `systemctl disable --now ${SERVICE} || true`,
    `rm -f ${NM_UNMANAGED_CONF}`,
    'nmcli general reload',
    `nmcli device set ${iface} managed yes`,
    `nmcli -w 8 device connect ${iface} || true`
  ].join('\n')
  // Bounded: a stuck nmcli/pkexec must never wedge the release.
  await new Promise<void>((resolve) => {
    const proc = spawn('pkexec', ['bash', '-c', script], { stdio: 'ignore' })
    const timer = setTimeout(() => proc.kill('SIGKILL'), 12_000)
    const done = (): void => {
      clearTimeout(timer)
      resolve()
    }
    proc.on('close', done)
    proc.on('error', done)
  })
}

let installing = false

/** Own the interface when dedicated or while wireless CarPlay/AA is on, else return it. */
export async function reconcileWifiAp(config: Config, window?: BrowserWindow): Promise<void> {
  if (process.platform !== 'linux') return
  if (!apWanted(config)) {
    await releaseInterface(config)
    return
  }

  if (needsInstall()) {
    if (installing) return
    installing = true
    try {
      if (window) {
        const { response } = await dialog.showMessageBox(window, {
          type: 'question',
          title: 'Wireless Projection — Wi-Fi AP',
          message: 'LIVI manages a Wi-Fi access point for wireless projection.',
          detail: `Installs a systemd unit (${UNIT_PATH}) and a sudoers rule (${SUDOERS_PATH}) so LIVI can take the interface for the AP and hand it back to your system afterwards.`,
          buttons: ['Install', 'Skip'],
          defaultId: 0,
          cancelId: 1
        })
        if (response !== 0) {
          installing = false
          return
        }
      }
      await installPrivileged()
      writeFileSync(markerPath(), INSTALL_VERSION)
    } catch (err) {
      console.error('[wifiApUnit] install failed:', err)
      installing = false
      return
    }
    installing = false
  }

  const sc = systemctlPath()
  await sudo([sc, config.wifiDedicatedInterface ? 'enable' : 'disable', SERVICE])
  await sudo([sc, 'start', SERVICE])
}

/** before-quit: keep the AP only when dedicated, otherwise return the interface. */
export async function releaseWifiApForQuit(config: Config): Promise<void> {
  if (process.platform !== 'linux') return
  if (config.wifiDedicatedInterface) return
  await sudo([systemctlPath(), 'stop', SERVICE])
}
