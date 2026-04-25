/**
 * Hardware address auto-detection for Bluetooth and WiFi interfaces.
 *
 * Reads directly from the Linux sysfs virtual filesystem — no external
 * tools or hardcoded values needed. Works on any Linux system (Pi, CM5, x86).
 *
 * Layout:
 *   /sys/class/bluetooth/<iface>/address  → BT MAC  (e.g. DC:A6:32:E7:5A:FF)
 *   /sys/class/net/<iface>/address        → WiFi MAC (= AP BSSID when used as hostapd AP)
 *
 * Fallback order:
 *   1. Env var (AA_BT_MAC / AA_WIFI_BSSID)
 *   2. sysfs /sys/class/bluetooth|net
 *   3. `hciconfig` subprocess (for unusual kernel configs where sysfs is absent)
 */

import { execSync } from 'node:child_process'
import * as fs from 'node:fs'
import * as path from 'node:path'

const BT_SYSFS_DIR = '/sys/class/bluetooth'
const NET_SYSFS_DIR = '/sys/class/net'

/** Read and normalise a MAC address from a sysfs file. Returns null on any error. */
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

/** List entries in a sysfs directory, sorted. Returns [] on error. */
function listSysfsDir(dir: string): string[] {
  try {
    return fs.readdirSync(dir).sort()
  } catch {
    return []
  }
}

/** Fallback: parse `hciconfig <iface>` output for BD Address. */
function readBtMacFromHciconfig(iface = 'hci0'): string | null {
  try {
    const out = execSync(`hciconfig ${iface} 2>/dev/null`, { encoding: 'utf8', timeout: 2000 })
    const m = out.match(/BD Address:\s*([0-9A-Fa-f]{2}(?::[0-9A-Fa-f]{2}){5})/)
    return m ? m[1]!.toUpperCase() : null
  } catch {
    return null
  }
}

/**
 * Detect the Bluetooth adapter MAC address.
 *
 * Tries each hci* interface in /sys/class/bluetooth in order, then falls back
 * to `hciconfig`. Override via AA_BT_MAC env var.
 *
 * @param iface  Force a specific interface name (e.g. "hci0")
 */
export function detectBtMac(iface?: string): string | undefined {
  if (process.env['AA_BT_MAC']) return process.env['AA_BT_MAC']

  const candidates = iface ? [iface] : listSysfsDir(BT_SYSFS_DIR).filter((n) => n.startsWith('hci'))

  for (const name of candidates) {
    const mac = readSysfsMac(path.join(BT_SYSFS_DIR, name, 'address'))
    if (mac) {
      console.log(`[hwaddr] BT MAC detected from sysfs: ${mac} (${name})`)
      return mac
    }
  }

  // Fallback: hciconfig (covers kernels that don't expose sysfs address)
  const hciFace = iface ?? 'hci0'
  const mac = readBtMacFromHciconfig(hciFace)
  if (mac) {
    console.log(`[hwaddr] BT MAC detected from hciconfig: ${mac} (${hciFace})`)
    return mac
  }

  console.warn('[hwaddr] Could not detect BT MAC. Set AA_BT_MAC env var if needed.')
  return undefined
}

/**
 * Detect the WiFi AP BSSID (= MAC address of the WiFi interface).
 *
 * On a Pi/CM5 running hostapd on wlan0, the BSSID is exactly
 * /sys/class/net/wlan0/address. Override via AA_WIFI_BSSID env var.
 *
 * @param iface  Force a specific interface name (e.g. "wlan0")
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
