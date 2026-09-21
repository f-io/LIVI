/**
 * Hardware address auto-detection for Bluetooth and WiFi interfaces.
 */

import { execSync } from 'node:child_process'
import * as fs from 'node:fs'
import * as path from 'node:path'

const BT_SYSFS_DIR = '/sys/class/bluetooth'
const NET_SYSFS_DIR = '/sys/class/net'
const DONGLE_LINK = 'livi-link'

function readSysfsMac(filePath: string): string | null {
  try {
    const raw = fs.readFileSync(filePath, 'utf8').trim()
    if (/^([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}$/.test(raw)) {
      return raw.toUpperCase()
    }
    return null
  } catch {
    return null
  }
}

/** Whether the controller sits on vhci, as the LIVI Link's does. */
export function isTunnelledBtAdapter(hci: string): boolean {
  try {
    return fs.realpathSync(path.join(BT_SYSFS_DIR, hci)).includes('/devices/virtual/')
  } catch {
    return false
  }
}

function listSysfsDir(dir: string): string[] {
  try {
    return fs.readdirSync(dir).sort()
  } catch {
    return []
  }
}

// These tools exist only on Linux.
function readBtMacFromHciconfig(iface = 'hci0'): string | null {
  try {
    const out = execSync(`hciconfig ${iface}`, {
      encoding: 'utf8',
      timeout: 2000,
      stdio: ['ignore', 'pipe', 'ignore']
    })
    const m = out.match(/BD Address:\s*([0-9A-Fa-f]{2}(?::[0-9A-Fa-f]{2}){5})/)
    return m ? m[1]!.toUpperCase() : null
  } catch {
    return null
  }
}

function readBtMacFromBusctl(iface = 'hci0'): string | null {
  try {
    const out = execSync(
      `busctl --system get-property org.bluez /org/bluez/${iface} org.bluez.Adapter1 Address`,
      { encoding: 'utf8', timeout: 2000, stdio: ['ignore', 'pipe', 'ignore'] }
    )
    // Output format: s "AA:BB:CC:DD:EE:FF"
    const m = out.match(/"([0-9A-Fa-f]{2}(?::[0-9A-Fa-f]{2}){5})"/)
    return m ? m[1]!.toUpperCase() : null
  } catch {
    return null
  }
}

export function detectBtMac(iface?: string): string | undefined {
  if (process.env['AA_BT_MAC']) return process.env['AA_BT_MAC']
  if (iface === DONGLE_LINK) {
    iface = listSysfsDir(BT_SYSFS_DIR).find(isTunnelledBtAdapter)
    if (!iface) return undefined
  }

  const candidates = iface ? [iface] : listSysfsDir(BT_SYSFS_DIR).filter((n) => n.startsWith('hci'))

  for (const name of candidates) {
    const mac = readSysfsMac(path.join(BT_SYSFS_DIR, name, 'address'))
    if (mac) {
      console.log(`[hwaddr] BT MAC detected from sysfs: ${mac} (${name})`)
      return mac
    }
  }

  // Pi OS / newer kernels don't expose hci0/address in sysfs — go via BlueZ D-Bus.
  const hciFace = iface ?? 'hci0'
  const macBusctl = readBtMacFromBusctl(hciFace)
  if (macBusctl) {
    console.log(`[hwaddr] BT MAC detected from busctl: ${macBusctl} (${hciFace})`)
    return macBusctl
  }

  // Last resort: hciconfig (often not installed on modern distros).
  const macHci = readBtMacFromHciconfig(hciFace)
  if (macHci) {
    console.log(`[hwaddr] BT MAC detected from hciconfig: ${macHci} (${hciFace})`)
    return macHci
  }

  console.warn('[hwaddr] Could not detect BT MAC. Set AA_BT_MAC env var if needed.')
  return undefined
}

/**
 * Detect the WiFi AP BSSID (= MAC address of the WiFi interface).
 */
export function detectWifiBssid(iface?: string): string | undefined {
  if (process.env['AA_WIFI_BSSID']) return process.env['AA_WIFI_BSSID']

  const candidates = iface
    ? [iface]
    : listSysfsDir(NET_SYSFS_DIR).filter((n) => n.startsWith('wlan'))

  for (const name of candidates) {
    const mac = readSysfsMac(path.join(NET_SYSFS_DIR, name, 'address'))
    if (mac) {
      console.log(`[hwaddr] WiFi BSSID detected: ${mac} (${name})`)
      return mac
    }
  }

  console.warn('[hwaddr] Could not detect WiFi BSSID. Set AA_WIFI_BSSID env var if needed.')
  return undefined
}
