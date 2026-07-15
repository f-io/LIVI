import * as net from 'node:net'
import type { MfiSigner } from './stack/mfiSigner'

/**
 * Client for the livi-bt helper's CarPlay control socket (/tmp/cp-bt.sock).
 *
 * One Unix-domain socket carries every LIVI <-> helper exchange, mirroring AA's
 * aa-bt.sock: line-JSON RPC for MFi (certificate/sign) and BlueZ device control
 * (disconnect), plus a raw-byte "tunnel" connection for iAP2-over-CarPlay. Binary
 * payloads travel base64-encoded inside the JSON. Implements MfiSigner so CpStack
 * reaches the coprocessor through the same channel as everything else.
 */

export const CP_BT_SOCK_PATH = '/tmp/cp-bt.sock'

type RpcResponse = { ok: true; data?: string } | { ok: false; error: string }

export class CpHelperSockError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'CpHelperSockError'
  }
}

export class CpHelperSock implements MfiSigner {
  constructor(private readonly path: string = CP_BT_SOCK_PATH) {}

  private request(line: string, timeoutMs = 8000): Promise<RpcResponse> {
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
        settle(() => reject(new CpHelperSockError(`cp-bt sock timeout after ${timeoutMs}ms`)))
      }, timeoutMs)

      sock.on('connect', () => sock.write(line + '\n'))
      sock.on('data', (data: Buffer) => {
        buf += data.toString('utf8')
        const nl = buf.indexOf('\n')
        if (nl < 0) return
        settle(() => {
          try {
            resolve(JSON.parse(buf.slice(0, nl)) as RpcResponse)
          } catch (e) {
            reject(new CpHelperSockError(`cp-bt sock bad json: ${(e as Error).message}`))
          }
        })
      })
      sock.on('error', (err: Error) =>
        settle(() => reject(new CpHelperSockError(`cp-bt sock error: ${err.message}`)))
      )
      sock.on('end', () => {
        if (!settled)
          settle(() => reject(new CpHelperSockError('cp-bt sock closed without response')))
      })
    })
  }

  private async requestData(line: string): Promise<Buffer> {
    const res = await this.request(line)
    if (!res.ok) throw new CpHelperSockError(res.error)
    return Buffer.from(res.data ?? '', 'base64')
  }

  /** MFi accessory certificate, read from the coprocessor by the helper. */
  certificate(): Promise<Buffer> {
    return this.requestData('certificate')
  }

  /** Sign a digest with the coprocessor's private key. */
  sign(digest: Buffer): Promise<Buffer> {
    return this.requestData(`sign ${digest.toString('base64')}`)
  }

  /** Tear down the phone's Bluetooth ACL (drops A2DP) after disableBluetooth. */
  async disconnectBt(mac: string): Promise<void> {
    const res = await this.request(`disconnect ${mac}`)
    if (!res.ok) throw new CpHelperSockError(res.error)
  }

  /** Hand NMEA sentences to the iAP2 stack for a LocationInformation update (base64,
   *  since a fix carries embedded CR/LF). Best-effort: dropped if no phone subscribed. */
  async sendLocation(nmea: string): Promise<void> {
    await this.request(`location ${Buffer.from(nmea, 'utf8').toString('base64')}`)
  }

  /** Hand vehicle status (range km / outside °C / low-range warning) to the iAP2 stack
   *  for a VehicleStatusUpdate. Best-effort: dropped if no phone subscribed. */
  async sendVehicleStatus(status: {
    range?: number
    outsideTemperature?: number
    rangeWarning?: boolean
  }): Promise<void> {
    await this.request(`vehicle-status ${JSON.stringify(status)}`)
  }

  /** Register / unregister the Android Auto BT profile in the running helper, so
   *  wireless AA can be toggled without restarting the helper or the Wi-Fi AP. */
  async setAaWireless(enabled: boolean): Promise<void> {
    const res = await this.request(`set-aa ${enabled ? '1' : '0'}`)
    if (!res.ok) throw new CpHelperSockError(res.error)
  }

  /** Register / unregister the wireless CarPlay iAP2 BT profile in the running
   *  helper (wired carkit stays up), toggled without restarting the helper or AP. */
  async setCpWireless(enabled: boolean): Promise<void> {
    const res = await this.request(`set-cp ${enabled ? '1' : '0'}`)
    if (!res.ok) throw new CpHelperSockError(res.error)
  }

  subscribeEvents(onEvent: (ev: Record<string, unknown>) => void): { close: () => void } {
    let closed = false
    let sock: net.Socket | null = null
    const connect = (): void => {
      if (closed) return
      const s = net.createConnection(this.path)
      sock = s
      let buf = ''
      s.on('connect', () => s.write('subscribe\n'))
      s.on('data', (d: Buffer) => {
        buf += d.toString('utf8')
        let nl = buf.indexOf('\n')
        while (nl >= 0) {
          const line = buf.slice(0, nl).trim()
          buf = buf.slice(nl + 1)
          if (line) {
            try {
              onEvent(JSON.parse(line) as Record<string, unknown>)
            } catch {
              /* ignore malformed push line */
            }
          }
          nl = buf.indexOf('\n')
        }
      })
      s.on('error', () => {
        /* 'close' follows and schedules the retry */
      })
      s.on('close', () => {
        if (closed) return
        sock = null
        setTimeout(connect, 1000)
      })
    }
    connect()
    return {
      close: (): void => {
        closed = true
        try {
          sock?.destroy()
        } catch {
          /* already torn down */
        }
      }
    }
  }
}
