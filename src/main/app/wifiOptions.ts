import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { resolveHelperBin } from '@main/services/projection/driver/helper/helperSupervisor'

// iw and regdbdump live in sbin, which a desktop session does not carry in its PATH.
function tool(name: string): string {
  for (const dir of ['/usr/sbin', '/sbin', '/usr/bin', '/bin']) {
    if (existsSync(`${dir}/${name}`)) return `${dir}/${name}`
  }
  return name
}

function run(cmd: string, args: string[]): string | null {
  if (process.platform !== 'linux') return null
  try {
    return execFileSync(cmd, args, { encoding: 'utf8', timeout: 3000 })
  } catch {
    return null
  }
}

// Net devices that expose a wireless directory in sysfs are the Wi-Fi interfaces.
export function listWifiInterfaces(): string[] {
  if (process.platform !== 'linux') return []
  try {
    return readdirSync('/sys/class/net')
      .filter((iface) => existsSync(`/sys/class/net/${iface}/wireless`))
      .sort()
  } catch {
    return []
  }
}

export function listBtAdapters(): string[] {
  if (process.platform !== 'linux') return []
  try {
    return readdirSync('/sys/class/bluetooth')
      .filter((n) => /^hci\d+$/.test(n))
      .sort()
  } catch {
    return []
  }
}

// Without the radio's own list, only what every regulatory domain allows: no DFS, no UNII-3.
const FALLBACK_CHANNELS_24 = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
const FALLBACK_CHANNELS_5 = [36, 40, 44, 48]

const FALLBACK_COUNTRIES = [
  'DE',
  'AT',
  'CH',
  'NL',
  'BE',
  'LU',
  'FR',
  'GB',
  'IE',
  'IT',
  'ES',
  'PT',
  'PL',
  'CZ',
  'SK',
  'HU',
  'RO',
  'BG',
  'GR',
  'HR',
  'SI',
  'DK',
  'SE',
  'NO',
  'FI',
  'IS',
  'EE',
  'LV',
  'LT',
  'US',
  'CA',
  'MX',
  'BR',
  'AU',
  'NZ',
  'JP',
  'KR',
  'CN',
  'IN',
  'ZA',
  'AE',
  'TR',
  'UA'
]

// Non-DFS standard AP channels; DFS (52-64, 100-144).
const ALLOWED_CHANNELS_24 = new Set([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13])
const ALLOWED_CHANNELS_5 = new Set([36, 40, 44, 48, 149, 153, 157, 161, 165])

// Under this a band is a short range device allowance, not a WLAN one.
const MIN_AP_DBM = 17

type Channel = { ch: number; freq: number; flags: string; dbm: number }
type Radio = { country: string; channels: Channel[] }

/** The radio behind an interface. */
function phyOf(iface: string): string {
  if (!iface) return ''
  try {
    return readFileSync(`/sys/class/net/${iface}/phy80211/name`, 'utf8').trim()
  } catch {
    return ''
  }
}

/** Every frequency of the radio behind `iface`, or of all of them when it is unknown. */
function radioChannels(iface: string): Radio | null {
  const out = run(resolveHelperBin(), ['--wifi-channels'])
  if (!out) return null
  const wanted = phyOf(iface)
  const radio: Radio = { country: '', channels: [] }
  let phy = ''
  for (const line of out.split('\n')) {
    const country = line.match(/^country\s+([A-Z]{2})/)
    if (country) {
      radio.country = country[1]
      continue
    }
    const named = line.match(/^phy\s+(\S+)/)
    if (named) {
      phy = named[1]
      continue
    }
    if (wanted && phy !== wanted) continue
    const chan = line.match(/^chan\s+(\d+)\s+(\d+)\s+(\S+)(?:\s+(\d+))?/)
    if (chan) {
      radio.channels.push({
        ch: Number(chan[1]),
        freq: Number(chan[2]),
        flags: chan[3],
        dbm: Number(chan[4] ?? 0)
      })
    }
  }
  return radio.channels.length ? radio : null
}

/** What this radio may transmit on, under the regulatory domain it is on right now. */
export function listWifiChannels(band: '2.4ghz' | '5ghz', country = '', iface = ''): number[] {
  const is5 = band === '5ghz'
  const allowed = is5 ? ALLOWED_CHANNELS_5 : ALLOWED_CHANNELS_24
  const fallback = is5 ? FALLBACK_CHANNELS_5 : FALLBACK_CHANNELS_24
  const radio = radioChannels(iface)
  if (!radio) return fallback
  // The driver knows the regulatory domain it is on, not the one that was just picked.
  if (country && radio.country && radio.country !== country.toUpperCase()) return fallback
  const chans = new Set<number>()
  for (const { ch, freq, flags, dbm } of radio.channels) {
    if (flags !== 'ok') continue
    if (dbm > 0 && dbm < MIN_AP_DBM) continue
    const inBand = is5 ? freq >= 4900 && freq < 5900 : freq >= 2400 && freq < 2500
    if (inBand && allowed.has(ch)) chans.add(ch)
  }
  return chans.size > 0 ? [...chans].sort((a, b) => a - b) : fallback
}

export function listWifiCountryCodes(): string[] {
  const out = run(tool('regdbdump'), ['/lib/firmware/regulatory.db'])
  if (out) {
    const codes = new Set<string>()
    for (const line of out.split('\n')) {
      const m = line.match(/^country ([A-Z]{2}):/)
      if (m && m[1] !== '00') codes.add(m[1])
    }
    if (codes.size > 0) return [...codes].sort()
  }
  return [...FALLBACK_COUNTRIES].sort()
}
