import type { ProjectionEvent } from '../services/types'
import { isPhoneLikeCod } from '../services/utils/isPhoneLikeCod'
import type { PairedDevice } from './BluezDeviceClient'

export type BtPairedRegistryDeps = {
  emit: (payload: ProjectionEvent) => void
  hasRenderer: () => boolean
}

/**
 * Owns the Bluetooth paired-list state: the host BlueZ raw source, the name/connected
 * cache derived from it, and the list the device picker consumes.
 */
export class BtPairedRegistry {
  private hostPairedRaw = ''
  private btNameByMac = new Map<string, string>()
  private connectedBtMac = ''

  constructor(private readonly deps: BtPairedRegistryDeps) {}

  getName(macUpper: string): string | undefined {
    return this.btNameByMac.get(macUpper)
  }

  getConnectedMac(): string {
    return this.connectedBtMac
  }

  // Fold a fresh BlueZ paired-device list into the host cache + raw list and emit the combined
  // list. Returns the raw connected MAC (pre prefer-override) and the phone subset the caller
  // still needs for its own bookkeeping.
  ingest(
    devices: PairedDevice[],
    opts: { cpClaimedBtMacs: Set<string>; preferMac?: string; keepHostRawIfEmpty: boolean }
  ): { connectedMac: string; phones: PairedDevice[] } {
    const phones = devices.filter((d) => isPhoneLikeCod(d.class))
    const connected =
      phones.find((d) => d.connected && !opts.cpClaimedBtMacs.has(d.mac.toUpperCase()))?.mac ?? ''
    this.btNameByMac = new Map(devices.map((d) => [d.mac.toUpperCase(), d.name || '']))

    const preferUp = Boolean(
      opts.preferMac &&
        phones.some((d) => d.connected && d.mac.toUpperCase() === opts.preferMac?.toUpperCase())
    )
    if (preferUp && opts.preferMac) this.connectedBtMac = opts.preferMac
    else if (connected) this.connectedBtMac = connected

    // Ignore transient empty responses to avoid UI flicker.
    if (!(devices.length === 0 && opts.keepHostRawIfEmpty)) {
      this.hostPairedRaw = devices.length
        ? devices.map((d) => `${d.mac}${d.name ?? ''}`).join('\n') + '\n'
        : ''
    }

    this.emitCombined()
    return { connectedMac: connected, phones }
  }

  // Emit the host paired list.
  emitCombined(): void {
    if (!this.deps.hasRenderer()) return
    const parse = (raw: string): Array<{ mac: string; line: string }> => {
      const out: Array<{ mac: string; line: string }> = []
      for (const line of raw.split('\n')) {
        const trimmed = line.replace(/\r$/, '').replace(/\0+$/g, '')
        if (trimmed.length < 17) continue
        const mac = trimmed.slice(0, 17).toUpperCase()
        if (!mac.includes(':')) continue
        out.push({ mac, line: trimmed })
      }
      return out
    }
    const all = parse(this.hostPairedRaw)
    const raw = all.length ? all.map((d) => d.line).join('\n') + '\n' : ''
    this.deps.emit({ type: 'bluetoothPairedList', payload: raw })
  }
}
