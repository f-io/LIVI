import net from 'node:net'
import os from 'node:os'
import type { Config } from '@shared/types/Config'

/** What the Wi-Fi interface and Bluetooth adapter lists carry for the dongle's own radios. */
export const DONGLE_LINK = 'livi-link'

const HOST = 'livi-link.local'
/** The dongle's USB link and its own access point both hand out addresses here. */
const LINK_SUBNET = '10.10.10.'
/** The access point answers here, the Bluetooth accessory on its own port. */
const PORT = 5001
const BT_PORT = 5005
/** Applying waits for the radio, and a 5 GHz start spends the first seconds scanning. */
const APPLY_MS = 30_000
const PROBE_MS = 1500

/**
 * Runs commands on one connection, in order, and gives up on the first one the dongle refuses.
 * Every answer ends in `ok` or `error <reason>`, so the next command goes out on the `ok`.
 */
function talk(commands: string[], timeoutMs = APPLY_MS, port = PORT): Promise<string[]> {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: HOST, port })
    const answers: string[] = []
    let buffer = ''
    let at = 0
    let done = false

    const finish = (err?: Error): void => {
      if (done) return
      done = true
      clearTimeout(timer)
      socket.destroy()
      if (err) reject(err)
      else resolve(answers)
    }
    // One timer for the whole exchange, since a name that does not resolve never reaches connect.
    const timer = setTimeout(() => finish(new Error('the dongle did not answer')), timeoutMs)

    socket.on('connect', () => socket.write(`${commands[at]}\n`))
    socket.on('data', (chunk) => {
      buffer += chunk.toString()
      for (let end = buffer.indexOf('\n'); end >= 0; end = buffer.indexOf('\n')) {
        const line = buffer.slice(0, end).trimEnd()
        buffer = buffer.slice(end + 1)
        if (line.startsWith('error ')) {
          finish(new Error(`${commands[at]}: ${line.slice(6)}`))
          return
        }
        if (line !== 'ok') {
          answers.push(line)
          continue
        }
        at += 1
        if (at >= commands.length) {
          finish()
          return
        }
        socket.write(`${commands[at]}\n`)
      }
    })
    socket.on('error', (err) => finish(err))
    socket.on('close', () => finish(new Error('the dongle closed the link')))
  })
}

/** What the dongle is told for this configuration. */
export function commandsFor(config: Config): string[] {
  if (config.wifiInterface !== DONGLE_LINK) {
    // Its radio would only sit next to the one actually in use.
    return ['off']
  }
  return [
    `set ssid ${config.carName || 'LIVI'}`,
    `set country ${config.country || 'DE'}`,
    `set channel ${config.wifiChannel || 36}`,
    `set passphrase ${config.wifiPassword || '12345678'}`,
    'apply',
    // Keeps the whole state across a reboot. Only a boot that reaches neither the USB link nor
    // the AP puts the default name back.
    'save'
  ]
}

export function btCommandsFor(config: Config): string[] {
  if (config.btAdapter !== DONGLE_LINK || !config.wirelessCpEnabled) return ['off']
  // Who may be paged comes from the paging list the helper hands over, not from here.
  return ['on']
}

/** The access point's MAC, as of the last exchange with the dongle. */
let apMac: string | null = null

/** What the phone is told to look for. Empty until the dongle has answered once. */
export function dongleApMac(): string | null {
  return apMac
}

function attached(): boolean {
  return Object.values(os.networkInterfaces())
    .flat()
    .some((address) => address?.address.startsWith(LINK_SUBNET))
}

/** Whether a LIVI Link is on the network and ready to be configured. */
export async function dongleApPresent(): Promise<boolean> {
  if (!attached()) return false
  // Right after the dongle is plugged in the route to it needs a moment, so one miss is no answer.
  let last = ''
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      await talk(['status'], PROBE_MS)
      return true
    } catch (e) {
      last = e instanceof Error ? e.message : String(e)
    }
  }
  console.log(`[dongleAp] ${HOST}:${PORT} did not answer: ${last}`)
  return false
}

/** Hands the dongle its settings when it is the chosen AP, and silences it when it is not. */
export async function reconcileDongleAp(config: Config): Promise<void> {
  if (!attached()) return
  try {
    const answers = await talk([...commandsFor(config), 'status'])
    apMac =
      answers
        .find((line) => line.startsWith('mac '))
        ?.slice(4)
        .trim() || apMac
  } catch (err) {
    // No dongle, or one that refused the change. Either way nothing local depends on it.
    console.warn('[dongleAp]', String(err))
  }
  try {
    await talk(btCommandsFor(config), APPLY_MS, BT_PORT)
  } catch (err) {
    console.warn('[dongleAp] bluetooth:', String(err))
  }
}

/** Switches off what LIVI switched on. The dongle brings both back by itself on its next boot. */
export async function releaseDongle(): Promise<void> {
  if (!attached()) return
  await Promise.allSettled([talk(['off'], PROBE_MS), talk(['off'], PROBE_MS, BT_PORT)])
}
