import * as net from 'net'

/**
 * Client for the Python aa-bluetooth.py IPC socket.
 *
 * Used by ProjectionService for BlueZ-side device management:
 * list_paired / connect / disconnect / remove.
 */

export const AA_BT_SOCK_PATH = '/tmp/aa-bt.sock'

export type PairedDevice = {
  mac: string
  name: string
  connected: boolean
  trusted: boolean
  class: number
  path: string
}

type ListPairedResponse = { ok: true; devices: PairedDevice[] } | { ok: false; error: string }
type ActionResponse = { ok: boolean; error?: string }

export class AaBtSockError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'AaBtSockError'
  }
}

export class AaBtSockClient {
  constructor(private readonly path: string = AA_BT_SOCK_PATH) {}

  private request(line: string, timeoutMs = 5000): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const sock = net.createConnection(this.path)
      let buf = ''
      let settled = false
      const settle = (fn: () => void): void => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        try {
          sock.destroy()
        } catch {
          /* already torn down */
        }
        fn()
      }
      const timer = setTimeout(() => {
        settle(() => reject(new AaBtSockError(`aa-bt sock timeout after ${timeoutMs}ms`)))
      }, timeoutMs)

      sock.on('connect', () => {
        sock.write(line + '\n')
      })
      sock.on('data', (data: Buffer) => {
        buf += data.toString('utf8')
        const nl = buf.indexOf('\n')
        if (nl < 0) return
        const json = buf.slice(0, nl)
        settle(() => {
          try {
            resolve(JSON.parse(json))
          } catch (e) {
            reject(new AaBtSockError(`aa-bt sock bad json: ${json} (${(e as Error).message})`))
          }
        })
      })
      sock.on('error', (err: Error) => {
        settle(() => reject(new AaBtSockError(`aa-bt sock error: ${err.message}`)))
      })
      sock.on('end', () => {
        if (!settled && !buf.includes('\n')) {
          settle(() => reject(new AaBtSockError('aa-bt sock closed without response')))
        }
      })
    })
  }

  /** Enumerate all paired BT devices known to BlueZ. */
  async listPaired(timeoutMs = 5000): Promise<PairedDevice[]> {
    const resp = (await this.request('list_paired', timeoutMs)) as ListPairedResponse
    if (!resp.ok) {
      throw new AaBtSockError(resp.error || 'list_paired failed')
    }
    return resp.devices
  }

  /** Initiate a BT connection to the given MAC (BlueZ Device1.Connect). */
  async connect(mac: string, timeoutMs = 32000): Promise<ActionResponse> {
    return (await this.request(`connect ${mac}`, timeoutMs)) as ActionResponse
  }

  /** Tear down the BT connection (BlueZ Device1.Disconnect). */
  async disconnect(mac: string, timeoutMs = 10000): Promise<ActionResponse> {
    return (await this.request(`disconnect ${mac}`, timeoutMs)) as ActionResponse
  }

  /** Unpair / forget the device (BlueZ Adapter1.RemoveDevice). */
  async remove(mac: string, timeoutMs = 10000): Promise<ActionResponse> {
    return (await this.request(`remove ${mac}`, timeoutMs)) as ActionResponse
  }

  /** Open a event subscription. */
  subscribe(
    onEvent: (ev: { event: string; mac?: string; path?: string }) => void,
    onClose?: () => void
  ): { close: () => void } {
    const sock = net.createConnection(this.path)
    let buf = ''
    let closed = false

    sock.on('connect', () => {
      sock.write('subscribe\n')
    })
    sock.on('data', (data: Buffer) => {
      buf += data.toString('utf8')
      let nl: number
      while ((nl = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, nl)
        buf = buf.slice(nl + 1)
        if (!line) continue
        try {
          const obj = JSON.parse(line)
          if (typeof obj === 'object' && obj && 'event' in obj) {
            onEvent(obj as { event: string; mac?: string; path?: string })
          }
        } catch {}
      }
    })
    const fireClose = (): void => {
      if (closed) return
      closed = true
      if (onClose) onClose()
    }
    sock.on('error', fireClose)
    sock.on('close', fireClose)

    return {
      close: () => {
        try {
          sock.destroy()
        } catch {}
      }
    }
  }
}
