import { execFileSync, spawn } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'
import { DONGLE_LINK } from '@main/services/link/dongleAp'
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
import type { Config } from '@shared/types/Config'
import { app, type BrowserWindow, dialog } from 'electron'
import { resolveHelperBin } from './helperSupervisor'
import { clearHelperRestaged, helperRestaged } from './staged'

const UNIT_PATH = '/etc/systemd/system/livi-wifi-ap.service'
const SUDOERS_PATH = '/etc/sudoers.d/99-LIVI-wifi-ap'
const SERVICE = 'livi-wifi-ap.service'
const NM_UNMANAGED_CONF = '/etc/NetworkManager/conf.d/99-livi-ap-unmanaged.conf'
const UNIT_TEMPLATE = 'livi-wifi-ap.service.template'
const SUDOERS_TEMPLATE = '99-LIVI-wifi-ap.sudoers.template'
// The installer writes the same marker, see livi_write_wifi_ap_unit in scripts/install/common.sh.
const MARKER = '.wifi-ap-install'

function helperPath(): string {
  return join(app.getPath('userData'), 'driver', 'livi-helperd')
}

function render(name: string): string {
  return asset(name)
    .replace(/__HELPER__/g, helperPath())
    .replace(/__USERNAME__/g, username())
    .replace(/__SYSTEMCTL__/g, systemctlPath())
}

function systemctlPath(): string {
  try {
    return execFileSync('which', ['systemctl'], { encoding: 'utf8' }).trim() || '/usr/bin/systemctl'
  } catch {
    return '/usr/bin/systemctl'
  }
}

function unitContent(): string {
  return render(UNIT_TEMPLATE)
}

function sudoersContent(): string {
  return render(SUDOERS_TEMPLATE)
}

function readFile(path: string): string {
  try {
    return existsSync(path) ? readFileSync(path, 'utf8') : ''
  } catch {
    return ''
  }
}

// The sudoers file is root-only. sudo is asked, and where it cannot answer the marker is.
function sudoersInstalled(): boolean {
  return sudoGrants(`restart ${SERVICE}`) || markerHolds(MARKER, sudoersContent())
}

function needsInstall(): boolean {
  return readFile(UNIT_PATH) !== unitContent() || !sudoersInstalled()
}

/** Hands both files to the helper, which the sudoers rule already lets us run as root. */
function installViaHelper(): boolean {
  let dir = ''
  try {
    const helper = resolveHelperBin()
    dir = mkdtempSync(join(os.tmpdir(), 'livi-ap-'))
    const unit = join(dir, 'unit')
    const rule = join(dir, 'rule')
    writeFileSync(unit, unitContent())
    writeFileSync(rule, sudoersContent())
    execFileSync('sudo', ['-n', helper, '--install-wifi-ap', unit, rule], {
      stdio: 'ignore',
      timeout: 20_000
    })
    return true
  } catch {
    return false
  } finally {
    if (dir) rmSync(dir, { recursive: true, force: true })
  }
}

function installPrivileged(): Promise<void> {
  return runAsRoot([
    `cat > ${UNIT_PATH} <<'EOF'`,
    unitContent().trimEnd(),
    'EOF',
    ...sudoersLines(SUDOERS_PATH, sudoersContent()),
    'systemctl daemon-reload'
  ])
}

function sudo(args: string[]): Promise<boolean> {
  return new Promise((resolve) => {
    const proc = spawn('sudo', ['-n', ...args], { stdio: 'ignore' })
    const timer = setTimeout(() => proc.kill('SIGKILL'), 12_000)
    const done = (ok: boolean): void => {
      clearTimeout(timer)
      resolve(ok)
    }
    proc.on('close', (code) => done(code === 0))
    proc.on('error', () => done(false))
  })
}

function apWanted(config: Config): boolean {
  if (config.wifiInterface === DONGLE_LINK) return false
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
async function releaseInterface(): Promise<void> {
  const taken =
    existsSync(NM_UNMANAGED_CONF) ||
    (await cmdOk('systemctl', ['is-active', '--quiet', SERVICE])) ||
    (await cmdOk('systemctl', ['is-enabled', '--quiet', SERVICE]))
  if (!taken) return
  const sc = systemctlPath()
  await sudo([sc, 'stop', SERVICE])
  await sudo([sc, 'disable', SERVICE])
  await sudo([helperPath(), '--wifi-ap-teardown'])
}

let installing = false

/** Own the interface when dedicated or while wireless CarPlay/AA is on, else return it. */
export async function reconcileWifiAp(config: Config, window?: BrowserWindow): Promise<void> {
  if (process.platform !== 'linux') return
  if (!apWanted(config)) {
    await releaseInterface()
    return
  }

  let install: boolean
  try {
    install = needsInstall()
  } catch (err) {
    console.error('[wifiApUnit] cannot read the unit templates:', err)
    return
  }

  if (install) {
    if (installing) return
    installing = true
    try {
      if (!installViaHelper()) {
        if (!pkexecAvailable()) {
          console.warn(
            `[wifiApUnit] cannot install ${UNIT_PATH} and ${SUDOERS_PATH}: the helper is not ` +
              'reachable as root and pkexec is missing. Run the LIVI install script on this host.'
          )
          return
        }
        if (window) {
          const { response } = await dialog.showMessageBox(window, {
            type: 'question',
            title: 'Wireless Projection — Wi-Fi AP',
            message: 'Install the Wi-Fi access point service?',
            detail: 'Wireless CarPlay and Android Auto need it.',
            buttons: ['Install', 'Skip'],
            defaultId: 0,
            cancelId: 1
          })
          if (response !== 0) return
        }
        await installPrivileged()
      }
      writeMarker(MARKER, sudoersContent())
    } catch (err) {
      console.error('[wifiApUnit] install failed:', err)
      return
    } finally {
      installing = false
    }
  }

  const sc = systemctlPath()
  await sudo([sc, config.wifiDedicatedInterface ? 'enable' : 'disable', SERVICE])
  // The service holds the old binary open until it is restarted, and start does nothing to a
  // running unit.
  const restaged = helperRestaged()
  await sudo([sc, restaged ? 'restart' : 'start', SERVICE])
  if (restaged) clearHelperRestaged()
}

const AP_SETTLE_TRIES = 15
const AP_SETTLE_MS = 2000

let report: ((patch: Partial<Config>) => void) | null = null

/** Where a corrected channel is written back to. */
export function setWifiApReport(fn: (patch: Partial<Config>) => void): void {
  report = fn
}

/** What the access point ended up on. */
function runningWifiAp(): { running: boolean; channel: number; width: number } | null {
  let out = ''
  try {
    out =
      execFileSync(resolveHelperBin(), ['--wifi-ap-status'], {
        encoding: 'utf8',
        timeout: 3000
      }) || ''
  } catch {
    return null
  }
  const value = (key: string): string =>
    out
      .split('\n')
      .find((l) => l.startsWith(`${key} `))
      ?.slice(key.length + 1) ?? ''
  const channel = Number(value('channel'))
  if (!channel) return null
  return { running: value('running') === 'true', channel, width: Number(value('width')) }
}

/** Hands the AP service a settings change. It reads channel, width and the rest once at start. */
export async function restartWifiAp(config: Config): Promise<void> {
  if (process.platform !== 'linux') return
  if (!apWanted(config) || needsInstall()) return
  await sudo([systemctlPath(), 'restart', SERVICE])
  void settleWifiAp(config)
}

/** Writes back what the service ended up on. Only after a start that carried this config. */
export async function settleWifiAp(config: Config): Promise<void> {
  for (let i = 0; i < AP_SETTLE_TRIES; i += 1) {
    const live = runningWifiAp()
    if (live?.running) {
      const patch: Partial<Config> = {}
      if (live.channel !== config.wifiChannel) patch.wifiChannel = live.channel
      if (live.width > 0 && live.width !== config.wifiChannelWidth) {
        patch.wifiChannelWidth = live.width
      }
      if (Object.keys(patch).length > 0) {
        console.log(
          `[wifiApUnit] channel ${config.wifiChannel} at ${config.wifiChannelWidth}MHz was refused, it runs on ${live.channel} at ${live.width}MHz`
        )
        report?.(patch)
      }
      return
    }
    await new Promise((done) => setTimeout(done, AP_SETTLE_MS))
  }
}

/** before-quit: keep the AP only when dedicated, otherwise return the interface. */
export async function releaseWifiApForQuit(config: Config): Promise<void> {
  if (process.platform !== 'linux') return
  if (config.wifiDedicatedInterface) return
  await sudo([systemctlPath(), 'stop', SERVICE])
}
