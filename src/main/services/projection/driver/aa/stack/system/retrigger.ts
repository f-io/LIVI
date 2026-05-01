/**
 * RFCOMM retrigger client.
 *
 * After a cleanup TCP session (phone sends AUDIO_FOCUS_RELEASE + FIN),
 * the phone will not reconnect unless the HU re-sends WifiStartRequest over
 * the still-open RFCOMM connection.
 *
 * aa-bluetooth.py holds the RFCOMM socket open and listens on a Unix domain
 * socket for retrigger requests. This module sends that request.
 */

import * as net from 'node:net'

const RETRIGGER_SOCK = '/tmp/aa-rfcomm-retrigger.sock'
const TIMEOUT_MS = 3_000

/**
 * Ask aa-bluetooth.py to re-send WifiStartRequest over the open RFCOMM socket.
 * Resolves to true if the retrigger was acknowledged, false otherwise.
 * Never throws — failures are logged and swallowed.
 */
export function triggerRfcommRetrigger(): Promise<boolean> {
  return new Promise((resolve) => {
    const sock = net.createConnection(RETRIGGER_SOCK)

    const timer = setTimeout(() => {
      sock.destroy()
      console.warn('[retrigger] timeout — aa-bluetooth.py did not respond')
      resolve(false)
    }, TIMEOUT_MS)

    sock.once('connect', () => {
      sock.write(Buffer.from('R')) // any byte triggers retrigger
    })

    let reply = ''
    sock.on('data', (chunk) => {
      reply += chunk.toString()
    })

    sock.once('close', () => {
      clearTimeout(timer)
      const ok = reply.startsWith('OK')
      if (ok) {
        console.log('[retrigger] ✓ WifiStartRequest re-sent — phone should reconnect')
      } else {
        console.warn('[retrigger] ✗ retrigger failed or no RFCOMM socket available')
      }
      resolve(ok)
    })

    sock.once('error', (err) => {
      clearTimeout(timer)
      if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
        console.warn('[retrigger] aa-bluetooth.py not running (socket not found)')
      } else {
        console.warn('[retrigger] error:', err.message)
      }
      resolve(false)
    })
  })
}
