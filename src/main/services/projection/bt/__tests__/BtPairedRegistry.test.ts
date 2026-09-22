import type { PairedDevice } from '../BluezDeviceClient'
import { BtPairedRegistry } from '../BtPairedRegistry'

type Payload = { type: string; payload: unknown }

function make(hasRenderer = true) {
  const emit = vi.fn<[Payload], void>()
  const reg = new BtPairedRegistry({ emit, hasRenderer: () => hasRenderer })
  return { reg, emit }
}

const PHONE_COD = 0x5a020c
const AUDIO_COD = 0x240404

const dev = (mac: string, over: Partial<PairedDevice> = {}): PairedDevice => ({
  mac,
  name: 'Dev',
  connected: false,
  trusted: true,
  class: PHONE_COD,
  path: `/org/bluez/${mac}`,
  ...over
})

describe('BtPairedRegistry.emitCombined', () => {
  const opts = { cpClaimedBtMacs: new Set<string>(), keepHostRawIfEmpty: false }

  test('emits the host paired list as a bluetoothPairedList event', () => {
    const { reg, emit } = make()
    reg.ingest([dev('AA:BB:CC:DD:EE:FF', { name: 'Pixel' })], opts)
    expect(emit.mock.calls.at(-1)![0]).toEqual({
      type: 'bluetoothPairedList',
      payload: 'AA:BB:CC:DD:EE:FFPixel\n'
    })
  })

  test('empty host list emits an empty string', () => {
    const { reg, emit } = make()
    reg.ingest([], opts)
    expect(emit.mock.calls.at(-1)![0].payload).toBe('')
  })

  test('does not emit when no renderer is attached', () => {
    const { reg, emit } = make(false)
    reg.ingest([dev('AA:BB:CC:DD:EE:FF', { name: 'Pixel' })], opts)
    expect(emit).not.toHaveBeenCalled()
  })

  test('drops paired entries whose leading field is not a colon MAC', () => {
    const { reg, emit } = make()
    reg.ingest([dev('AABBCCDDEEFF0011X', { name: 'Y' })], opts)
    expect(emit.mock.calls.at(-1)![0].payload).toBe('')
  })
})

describe('BtPairedRegistry.ingest', () => {
  const noCp = { cpClaimedBtMacs: new Set<string>(), keepHostRawIfEmpty: false }

  test('builds the upper-cased name cache and emits the combined list', () => {
    const { reg, emit } = make()
    reg.ingest([dev('aa:bb:cc:dd:ee:ff', { name: 'Pixel' })], noCp)
    expect(reg.getName('AA:BB:CC:DD:EE:FF')).toBe('Pixel')
    expect(emit.mock.calls[0][0].payload).toBe('aa:bb:cc:dd:ee:ffPixel\n')
  })

  test('picks the connected phone and skips cp-claimed macs', () => {
    const { reg } = make()
    const res = reg.ingest(
      [
        dev('AA:BB:CC:DD:EE:FF', { connected: true }),
        dev('11:22:33:44:55:66', { connected: true })
      ],
      { cpClaimedBtMacs: new Set(['AA:BB:CC:DD:EE:FF']), keepHostRawIfEmpty: false }
    )
    expect(res.connectedMac).toBe('11:22:33:44:55:66')
    expect(reg.getConnectedMac()).toBe('11:22:33:44:55:66')
  })

  test('preferMac overrides connectedMac cache but returns the raw connected', () => {
    const { reg } = make()
    const res = reg.ingest(
      [
        dev('AA:BB:CC:DD:EE:FF', { connected: true }),
        dev('11:22:33:44:55:66', { connected: true })
      ],
      { cpClaimedBtMacs: new Set(), preferMac: '11:22:33:44:55:66', keepHostRawIfEmpty: false }
    )
    expect(res.connectedMac).toBe('AA:BB:CC:DD:EE:FF')
    expect(reg.getConnectedMac()).toBe('11:22:33:44:55:66')
  })

  test('returns only the phone-like subset', () => {
    const { reg } = make()
    const res = reg.ingest(
      [dev('AA:BB:CC:DD:EE:FF'), dev('11:22:33:44:55:66', { class: AUDIO_COD })],
      noCp
    )
    expect(res.phones.map((p) => p.mac)).toEqual(['AA:BB:CC:DD:EE:FF'])
  })

  test('keepHostRawIfEmpty keeps the previous host list on an empty response', () => {
    const { reg, emit } = make()
    reg.ingest([dev('AA:BB:CC:DD:EE:FF', { name: 'Pixel' })], noCp)
    emit.mockClear()

    reg.ingest([], { cpClaimedBtMacs: new Set(), keepHostRawIfEmpty: true })
    expect(emit.mock.calls[0][0].payload).toBe('AA:BB:CC:DD:EE:FFPixel\n')

    reg.ingest([], { cpClaimedBtMacs: new Set(), keepHostRawIfEmpty: false })
    expect(emit.mock.calls[1][0].payload).toBe('')
  })
})
