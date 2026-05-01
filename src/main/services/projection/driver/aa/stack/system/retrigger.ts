/**
 * RFCOMM retrigger client.
 */

import * as net from 'node:net'

const RETRIGGER_SOCK = '/tmp/aa-rfcomm-retrigger.sock'
const TIMEOUT_MS = 3_000

export function triggerRfcommRetrigger(): Promise<boolean> {
  return new Promise((resolve) => {
    const sock = net.createConnection(RETRIGGER_SOCK)

    const timer = setTimeout(() => {
      sock.destroy()
      console.warn('[retrigger] timeout — aa-bluetooth.py did not respond')
      resolve(false)
    }, TIMEOUT_MS)

    sock.once('connect', () => {
      sock.write(Buffer.from('R'))
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
