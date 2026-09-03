import * as net from 'net'

/**
 * Client for the livi-helperd IPC socket.
 *
 * Protocol-agnostic BlueZ device management (list_paired / connect / disconnect /
 * remove), shared by every projection path (Android Auto and CarPlay alike).
 */

export const BT_SOCK_PATH = '/tmp/aa-bt.sock'

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

export class BluezDeviceError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'BluezDeviceError'
  }
}

export class BluezDeviceClient {
  constructor(private readonly path: string = BT_SOCK_PATH) {}

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
        settle(() => reject(new BluezDeviceError(`aa-bt sock timeout after ${timeoutMs}ms`)))
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
            reject(new BluezDeviceError(`aa-bt sock bad json: ${json} (${(e as Error).message})`))
          }
        })
      })
      sock.on('error', (err: Error) => {
        settle(() => reject(new BluezDeviceError(`aa-bt sock error: ${err.message}`)))
      })
      sock.on('end', () => {
        if (!settled && !buf.includes('\n')) {
          settle(() => reject(new BluezDeviceError('aa-bt sock closed without response')))
        }
      })
    })
  }

  // Enumerate all paired BT devices known to BlueZ
  async listPaired(timeoutMs = 5000): Promise<PairedDevice[]> {
    const resp = (await this.request('list_paired', timeoutMs)) as ListPairedResponse
    if (!resp.ok) {
      throw new BluezDeviceError(resp.error || 'list_paired failed')
    }
    return resp.devices
  }

  // Initiate a BT connection to the given MAC (BlueZ Device1.ConnectProfile).
  async connect(mac: string, timeoutMs = 32000, uuid?: string): Promise<ActionResponse> {
    const line = uuid ? `connect ${mac} ${uuid}` : `connect ${mac}`
    return (await this.request(line, timeoutMs)) as ActionResponse
  }

  // Connect all auto-connect profiles (A2DP + HFP + HSP)
  async connectFull(mac: string, timeoutMs = 32000): Promise<ActionResponse> {
    return (await this.request(`connect-full ${mac}`, timeoutMs)) as ActionResponse
  }

  // Tear down one profile connection (BlueZ Device1.DisconnectProfile)
  async disconnectProfile(mac: string, uuid: string, timeoutMs = 10000): Promise<ActionResponse> {
    return (await this.request(`disconnect-profile ${mac} ${uuid}`, timeoutMs)) as ActionResponse
  }

  // Tear down the BT connection (BlueZ Device1.Disconnect)
  async disconnect(mac: string, timeoutMs = 10000): Promise<ActionResponse> {
    return (await this.request(`disconnect ${mac}`, timeoutMs)) as ActionResponse
  }

  // Unpair / forget the device (BlueZ Adapter1.RemoveDevice)
  async remove(mac: string, timeoutMs = 10000): Promise<ActionResponse> {
    return (await this.request(`remove ${mac}`, timeoutMs)) as ActionResponse
  }

  // Phones that already project over USB. The helper refuses to hand them the AP credentials
  async setWiredPhones(ids: string[], timeoutMs = 5000): Promise<ActionResponse> {
    return (await this.request(`wired-phones ${JSON.stringify(ids)}`, timeoutMs)) as ActionResponse
  }

  // Where the call audio goes: the pipeline's feed and stream id, nothing to stop
  async setScoSink(feed?: string, streamId?: number, timeoutMs = 5000): Promise<ActionResponse> {
    const arg = feed && streamId != null ? ` ${feed} ${streamId}` : ''
    return (await this.request(`sco-sink${arg}`, timeoutMs)) as ActionResponse
  }

  // Kick every associated Wi-Fi station off the AP
  async deauthApClients(timeoutMs = 5000): Promise<ActionResponse> {
    return (await this.request('deauth-ap', timeoutMs)) as ActionResponse
  }

  /** Mirrors the active session's play state into the helper's AVRCP player. */
  async setPlaybackStatus(state: 'playing' | 'paused' | 'stopped'): Promise<ActionResponse> {
    return (await this.request(`playback-status ${state}`)) as ActionResponse
  }

  // Open a event subscription
  subscribe(
    onEvent: (ev: {
      event: string
      mac?: string
      path?: string
      command?: string
      btMac?: string
      instanceId?: string
      usbSerial?: string
      up?: boolean
      pct?: number
      mtu?: number
      socket?: string
      peer?: string
    }) => void,
    onClose?: () => void,
    onOpen?: () => void
  ): { close: () => void } {
    const sock = net.createConnection(this.path)
    let buf = ''
    let closed = false

    sock.on('connect', () => {
      sock.write('subscribe\n')
      onOpen?.()
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
