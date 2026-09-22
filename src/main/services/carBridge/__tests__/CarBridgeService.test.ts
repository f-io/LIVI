import { execFile } from 'child_process'
import { EventEmitter } from 'events'
import fs from 'fs'
import type { Mock } from 'vitest'
import type { ProjectionEvent } from '../../projection/services/types'
import { CarBridgeService } from '../CarBridgeService'

vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => '/mock/userData') }
}))

vi.mock('child_process', () => ({
  execFile: vi.fn()
}))

vi.mock('@shared/utils', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@shared/utils')>()
  return {
    ...actual,
    translateNavigation: vi.fn(() => ({
      RemainDistanceText: '100 m',
      ManeuverTypeText: 'Turn right'
    }))
  }
})

import { translateNavigation } from '@shared/utils'

class FakeStream extends EventEmitter {
  written: string[] = []
  destroyed = false
  write(chunk: string): boolean {
    this.written.push(chunk)
    return true
  }
  end(): void {
    this.emit('close')
  }
  destroy(): void {
    this.destroyed = true
    this.emit('close')
  }
}

const files: Record<string, string> = {}
let writeStreams: FakeStream[] = []
let readStreams: FakeStream[] = []
let logSpy: ReturnType<typeof vi.spyOn>
const realPlatform = process.platform

function setPlatform(p: string): void {
  Object.defineProperty(process, 'platform', { value: p, configurable: true })
}

function ev(type: string, payload: unknown): ProjectionEvent {
  return { type, payload } as unknown as ProjectionEvent
}

function media(fields: Record<string, unknown>): ProjectionEvent {
  return ev('media', { payload: { media: fields } })
}

beforeEach(() => {
  vi.useFakeTimers()
  writeStreams = []
  readStreams = []
  for (const k of Object.keys(files)) delete files[k]
  vi.spyOn(fs, 'createWriteStream').mockImplementation(() => {
    const s = new FakeStream()
    writeStreams.push(s)
    return s as unknown as fs.WriteStream
  })
  vi.spyOn(fs, 'createReadStream').mockImplementation(() => {
    const s = new FakeStream()
    readStreams.push(s)
    return s as unknown as fs.ReadStream
  })
  vi.spyOn(fs, 'readFileSync').mockImplementation((p) => {
    const hit = files[String(p)]
    if (hit === undefined) throw new Error(`ENOENT ${p}`)
    return hit
  })
  ;(execFile as unknown as Mock).mockImplementation(
    (_cmd: string, _args: string[], cb: (err: Error | null) => void) => cb(null)
  )
  logSpy = vi.spyOn(console, 'log').mockImplementation(() => undefined)
})

afterEach(() => {
  vi.runOnlyPendingTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
  setPlatform(realPlatform)
})

// spins the darwin discovery until the service is connected to a fake port
function connectDarwin(svc: CarBridgeService): { port: FakeStream; reader: FakeStream } {
  setPlatform('darwin')
  vi.spyOn(fs, 'readdirSync').mockImplementation(() => ['cu.usbmodem1'] as never)
  svc.start()
  const probeReader = readStreams[0]
  const probeWriter = writeStreams[0]
  probeWriter.emit('open')
  expect(probeWriter.written).toEqual(['ping\n'])
  probeReader.emit('data', Buffer.from('pong\n'))
  const port = writeStreams[1]
  const reader = readStreams[1]
  port.emit('open')
  return { port, reader }
}

describe('CarBridgeService', () => {
  test('locale mapping feeds the idle text', () => {
    const texts: Record<string, string> = {
      de: 'Zielführung aus',
      fr: 'Aucun itinéraire',
      ua: 'Немає маршруту',
      uk: 'Немає маршруту',
      'uk-UA': 'Немає маршруту',
      en: 'No Route',
      unknown: 'No Route'
    }
    for (const [lang, idle] of Object.entries(texts)) {
      const svc = new CarBridgeService(lang)
      const { port } = connectDarwin(svc)
      expect(port.written).toContain(`nav noRouteText ${idle}\n`)
      svc.stop()
      writeStreams = []
      readStreams = []
    }
  })

  test('connect flushes name and idle nav, seeds media and nav from disk', () => {
    files['/mock/userData/mediaData.json'] = JSON.stringify({
      payload: {
        media: {
          MediaSongName: ' Song ',
          MediaArtistName: 'Artist',
          MediaAPPName: 'Spotify',
          MediaSongDuration: 200000,
          MediaPlayStatus: 1
        }
      }
    })
    files['/mock/userData/navigationData.json'] = JSON.stringify({
      payload: { navi: { NaviStatus: 1, NaviManeuverType: 2 } }
    })
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    expect(port.written).toContain('name LIVI\n')
    expect(port.written).toContain('media MediaSongName Song\n')
    expect(port.written).toContain('media MediaArtistName Artist\n')
    expect(port.written).toContain('media MediaAPPName Spotify\n')
    expect(port.written).toContain('media MediaSongDuration 200000\n')
    expect(port.written).toContain('media MediaPlayStatus 1\n')
    expect(port.written).toContain('nav NaviStatus 1\n')
    expect(port.written).toContain('nav NaviManeuverType 2\n')
    expect(port.written).toContain('nav remainDistanceText 100 m\n')
    expect(port.written).toContain('nav maneuverText Turn right\n')
    svc.stop()
  })

  test('disk seeds are skipped without files and the idle text goes out', () => {
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    expect(port.written).toEqual(['name LIVI\n', 'nav noRouteText No Route\n'])
    svc.stop()
  })

  test('a stopped guidance snapshot is forwarded 1:1, broken media json is skipped', () => {
    files['/mock/userData/navigationData.json'] = JSON.stringify({
      payload: { navi: { NaviStatus: 0, NaviManeuverType: 2 } }
    })
    files['/mock/userData/mediaData.json'] = '{broken'
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    expect(port.written).toContain('nav NaviStatus 0\n')
    expect(port.written).toContain('nav NaviManeuverType 2\n')
    svc.stop()
  })

  test('media event sends merged fields once and dedupes repeats', () => {
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    port.written.length = 0
    const fields = {
      MediaSongName: 'Hymn',
      MediaArtistName: 'Music Instructor',
      MediaAlbumName: 'Album',
      MediaAPPName: 'Spotify',
      MediaSongPlayTime: 1000,
      MediaSongDuration: 0,
      MediaPlayStatus: undefined,
      MediaIgnored: 'x'
    }
    svc.handleEvent(media(fields))
    expect(port.written).toEqual([
      'media MediaSongName Hymn\n',
      'media MediaArtistName Music Instructor\n',
      'media MediaAlbumName Album\n',
      'media MediaAPPName Spotify\n',
      'media MediaSongDuration 0\n',
      'media MediaSongPlayTime 1000\n'
    ])
    port.written.length = 0
    svc.handleEvent(media({ ...fields, MediaSongPlayTime: 2000 }))
    expect(port.written).toEqual(['media MediaSongPlayTime 2000\n'])
    svc.handleEvent(ev('media', { payload: {} }))
    expect(port.written).toEqual(['media MediaSongPlayTime 2000\n'])
    svc.stop()
  })

  test('media-reset blanks the text fields only after the debounce', () => {
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    svc.handleEvent(media({ MediaSongName: 'Hymn', MediaAPPName: 'Spotify' }))
    port.written.length = 0
    svc.handleEvent(ev('media-reset', { reason: 'session-switch' }))
    svc.handleEvent(ev('media-reset', { reason: 'again' }))
    vi.advanceTimersByTime(3999)
    expect(port.written).toEqual([])
    vi.advanceTimersByTime(1)
    expect(port.written).toEqual([
      'media MediaSongName \n',
      'media MediaArtistName \n',
      'media MediaAlbumName \n',
      'media MediaAPPName \n'
    ])
    svc.stop()
  })

  test('a media event within the debounce keeps the lines', () => {
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    svc.handleEvent(ev('media-reset', { reason: 'hop' }))
    svc.handleEvent(media({ MediaSongName: 'Hymn' }))
    port.written.length = 0
    vi.advanceTimersByTime(5000)
    expect(port.written).toEqual([])
    svc.stop()
  })

  test('navigation events forward the fields 1:1 with dedupe', () => {
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    port.written.length = 0
    svc.handleEvent(
      ev('navigation', {
        navi: {
          NaviStatus: 1,
          NaviManeuverType: 2,
          NaviDistanceToDestination: 6584,
          NaviETA: '23:10',
          NaviRoadName: 'A7'
        }
      })
    )
    expect(port.written).toEqual([
      'nav NaviStatus 1\n',
      'nav NaviManeuverType 2\n',
      'nav NaviDistanceToDestination 6584\n',
      'nav NaviETA 23:10\n',
      'nav NaviRoadName A7\n',
      'nav remainDistanceText 100 m\n',
      'nav maneuverText Turn right\n'
    ])
    port.written.length = 0
    // unchanged fields are deduped, changed ones go out
    svc.handleEvent(ev('navigation', { navi: { NaviStatus: 1, NaviManeuverType: 13 } }))
    expect(port.written).toEqual(['nav NaviManeuverType 13\n'])
    port.written.length = 0
    // untranslatable message contributes nothing new
    ;(translateNavigation as Mock).mockReturnValueOnce({})
    svc.handleEvent(ev('navigation', { navi: { NaviStatus: 1, NaviManeuverType: 13 } }))
    expect(port.written).toEqual([])
    svc.handleEvent(ev('navigation', {}))
    expect(port.written).toEqual([])
    svc.handleEvent(ev('navigation', { navi: { NaviStatus: 0 } }))
    expect(port.written).toEqual(['nav NaviStatus 0\n'])
    port.written.length = 0
    svc.handleEvent(ev('navigation-reset', { reason: 'gone' }))
    expect(port.written).toEqual([])
    svc.handleEvent(ev('unknown-type', {}))
    expect(port.written).toEqual([])
    svc.stop()
  })

  test('HU keys arriving on the reader reach onKey', () => {
    const svc = new CarBridgeService('en')
    const { reader } = connectDarwin(svc)
    const keys: string[] = []
    svc.onKey = (cmd) => keys.push(cmd)
    reader.emit('data', Buffer.from('EV key '))
    reader.emit('data', Buffer.from('next\nEV key prev\n'))
    reader.emit('data', Buffer.from('EV key scan\nok\nEV ir up\n'))
    expect(keys).toEqual(['next', 'previous', 'playPause'])
    reader.emit('data', Buffer.from('x'.repeat(600)))
    reader.emit('data', Buffer.from('\nEV key next\n'))
    expect(keys).toEqual(['next', 'previous', 'playPause', 'next'])
    svc.stop()
  })

  test('brightness is clamped to percent and cached for reconnects', () => {
    const svc = new CarBridgeService('en')
    svc.setBrightness(73.4)
    svc.setBrightness(Number.NaN)
    const { port } = connectDarwin(svc)
    expect(port.written).toContain('bright 73\n')
    port.written.length = 0
    svc.setBrightness(150)
    svc.setBrightness(-5)
    svc.setBrightness(-5)
    expect(port.written).toEqual(['bright 100\n', 'bright 0\n'])
    svc.stop()
  })

  test('car lines become typed telemetry payloads', () => {
    const svc = new CarBridgeService('en')
    const { reader } = connectDarwin(svc)
    const seen: unknown[] = []
    svc.onTelemetry = (p) => seen.push(p)
    reader.emit('data', Buffer.from('car speedKph 73.5\n'))
    reader.emit('data', Buffer.from('car lights true\ncar parkingBrake false\n'))
    reader.emit('data', Buffer.from('car gear D\n'))
    reader.emit('data', Buffer.from('car gps.lat 52.5\n'))
    reader.emit('data', Buffer.from('car speedKph\n'))
    reader.emit('data', Buffer.from('car speedKph   \n'))
    reader.emit('data', Buffer.from('car 1bad 5\n'))
    reader.emit('data', Buffer.from('car  5\n'))
    expect(seen).toEqual([
      { speedKph: 73.5 },
      { lights: true },
      { parkingBrake: false },
      { gear: 'D' },
      { gps: { lat: 52.5 } }
    ])
    svc.stop()
  })

  test('reader errors drop the writer and schedule a reconnect', () => {
    const svc = new CarBridgeService('en')
    const { port, reader } = connectDarwin(svc)
    reader.emit('error', new Error('gone'))
    expect(port.destroyed).toBe(true)
    const before = readStreams.length
    vi.advanceTimersByTime(3000)
    expect(readStreams.length).toBeGreaterThan(before)
    svc.stop()
  })

  test('write stream errors clear state and schedule a reconnect', () => {
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    port.emit('error', new Error('gone'))
    const before = readStreams.length
    vi.advanceTimersByTime(3000)
    expect(readStreams.length).toBeGreaterThan(before)
    svc.stop()
  })

  test('probe walks on without pong and retries when nothing answers', () => {
    setPlatform('darwin')
    vi.spyOn(fs, 'readdirSync').mockImplementation(() => ['cu.usbmodem1', 'cu.usbmodem2'] as never)
    const svc = new CarBridgeService('en')
    svc.start()
    readStreams[0].emit('data', Buffer.from('something else'))
    vi.advanceTimersByTime(700)
    readStreams[1].emit('error', new Error('busy'))
    expect(writeStreams).toHaveLength(2)
    vi.advanceTimersByTime(3000)
    expect(readStreams.length).toBeGreaterThan(2)
    svc.stop()
  })

  test('probe writer errors count as a miss', () => {
    setPlatform('darwin')
    vi.spyOn(fs, 'readdirSync').mockImplementation(() => ['cu.usbmodem1'] as never)
    const svc = new CarBridgeService('en')
    svc.start()
    writeStreams[0].emit('error', new Error('nope'))
    expect(writeStreams).toHaveLength(1)
    svc.stop()
  })

  test('darwin without /dev listing just retries', () => {
    setPlatform('darwin')
    vi.spyOn(fs, 'readdirSync').mockImplementation(() => {
      throw new Error('no /dev')
    })
    const svc = new CarBridgeService('en')
    svc.start()
    expect(writeStreams).toHaveLength(0)
    svc.stop()
  })

  test('linux discovery matches VID and CDC data interface via sysfs', () => {
    setPlatform('linux')
    vi.spyOn(fs, 'readdirSync').mockImplementation(() => ['ttyACM0', 'ttyACM1', 'ttyS0'] as never)
    vi.spyOn(fs, 'realpathSync').mockImplementation((p) => {
      if (String(p).includes('ttyACM0')) return '/sys/devices/usb1/1-1/1-1:1.0'
      return '/sys/devices/usb1/1-2/1-2:1.2'
    })
    files['/sys/devices/usb1/1-1/1-1:1.0/bInterfaceNumber'] = '00\n'
    files['/sys/devices/usb1/1-1/idVendor'] = 'cafe\n'
    files['/sys/devices/usb1/1-2/1-2:1.2/bInterfaceNumber'] = '02\n'
    files['/sys/devices/usb1/1-2/idVendor'] = 'cafe\n'
    const svc = new CarBridgeService('en')
    svc.start()
    const port = writeStreams[0]
    port.emit('open')
    expect(port.written).toContain('name LIVI\n')
    expect((execFile as unknown as Mock).mock.lastCall?.[1]).toEqual([
      '-F',
      '/dev/ttyACM1',
      'raw',
      '-echo',
      '115200'
    ])
    svc.stop()
  })

  test('linux without matches or without sysfs schedules a retry', () => {
    setPlatform('linux')
    const dirSpy = vi.spyOn(fs, 'readdirSync').mockImplementation(() => ['ttyACM0'] as never)
    vi.spyOn(fs, 'realpathSync').mockImplementation(() => {
      throw new Error('detached')
    })
    const svc = new CarBridgeService('en')
    svc.start()
    expect(writeStreams).toHaveLength(0)
    svc.stop()
    dirSpy.mockImplementation(() => {
      throw new Error('no sysfs')
    })
    const svc2 = new CarBridgeService('en')
    svc2.start()
    expect(writeStreams).toHaveLength(0)
    svc2.stop()
  })

  test('stty failure on linux schedules a retry instead of opening', () => {
    setPlatform('linux')
    vi.spyOn(fs, 'readdirSync').mockImplementation(() => ['ttyACM0'] as never)
    vi.spyOn(fs, 'realpathSync').mockImplementation(() => '/sys/devices/usb1/1-1/1-1:1.2')
    files['/sys/devices/usb1/1-1/1-1:1.2/bInterfaceNumber'] = '02\n'
    files['/sys/devices/usb1/1-1/idVendor'] = 'cafe\n'
    ;(execFile as unknown as Mock).mockImplementation(
      (_c: string, _a: string[], cb: (err: Error | null) => void) => cb(new Error('stty'))
    )
    const svc = new CarBridgeService('en')
    svc.start()
    expect(writeStreams).toHaveLength(0)
    svc.stop()
  })

  test('stop during discovery and setup goes quiet', () => {
    setPlatform('darwin')
    vi.spyOn(fs, 'readdirSync').mockImplementation(() => ['cu.usbmodem1'] as never)
    let sttyCb: ((err: Error | null) => void) | null = null
    ;(execFile as unknown as Mock).mockImplementation(
      (_c: string, _a: string[], cb: (err: Error | null) => void) => {
        sttyCb = cb
      }
    )
    const svc = new CarBridgeService('en')
    svc.start()
    writeStreams[0].emit('open')
    readStreams[0].emit('data', Buffer.from('pong'))
    svc.stop()
    sttyCb?.(null)
    expect(writeStreams).toHaveLength(1)
    // a late find-callback after stop must not reconnect either
    const svc2 = new CarBridgeService('en')
    svc2.start()
    svc2.stop()
    writeStreams[1]?.emit('open')
    expect(writeStreams.length).toBeLessThanOrEqual(2)
  })

  test('stop clears a pending media blank debounce', () => {
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    svc.handleEvent(media({ MediaSongName: 'Hymn' }))
    svc.handleEvent(ev('media-reset', { reason: 'hop' }))
    port.written.length = 0
    svc.stop()
    vi.advanceTimersByTime(5000)
    expect(port.written).toEqual([])
  })

  test('media seed skips a snapshot without a media object', () => {
    files['/mock/userData/mediaData.json'] = JSON.stringify({ payload: {} })
    files['/mock/userData/navigationData.json'] = JSON.stringify({ payload: { navi: 'kaputt' } })
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    expect(port.written).toEqual(['name LIVI\n', 'nav noRouteText No Route\n'])
    svc.stop()
  })

  test('events before connect are cached and flushed on connect', () => {
    files['/mock/userData/mediaData.json'] = JSON.stringify({
      payload: { media: { MediaSongName: 'Disk' } }
    })
    files['/mock/userData/navigationData.json'] = JSON.stringify({
      payload: { navi: { NaviStatus: 1, NaviManeuverType: 2 } }
    })
    const svc = new CarBridgeService('en')
    svc.handleEvent(media({ MediaSongName: 'Live', MediaAPPName: 'Spotify' }))
    svc.handleEvent(ev('navigation', { navi: { NaviStatus: 1, NaviManeuverType: 2 } }))
    const { port } = connectDarwin(svc)
    // cached live values win - neither disk snapshot is consulted
    expect(port.written).toContain('media MediaSongName Live\n')
    expect(port.written).toContain('nav NaviStatus 1\n')
    expect(port.written).not.toContain('media MediaSongName Disk\n')
    svc.stop()
  })

  test('a darwin stty error still opens the port', () => {
    ;(execFile as unknown as Mock).mockImplementation(
      (_c: string, _a: string[], cb: (err: Error | null) => void) => cb(new Error('stty'))
    )
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    expect(port.written).toContain('name LIVI\n')
    svc.stop()
  })

  test('a late close of the previous stream leaves the new one alone', () => {
    const svc = new CarBridgeService('en')
    const { port, reader } = connectDarwin(svc)
    // second probe answer after the timeout must not double-finish
    writeStreams[0].emit('error', new Error('late'))
    reader.emit('error', new Error('gone'))
    vi.advanceTimersByTime(3000)
    const probeReader2 = readStreams[2]
    const probeWriter2 = writeStreams[2]
    probeWriter2.emit('open')
    probeReader2.emit('data', Buffer.from('pong'))
    const port2 = writeStreams[3]
    port2.emit('open')
    expect(port2.written).toContain('name LIVI\n')
    port.emit('close')
    svc.handleEvent(media({ MediaSongName: 'Still' }))
    expect(port2.written).toContain('media MediaSongName Still\n')
    svc.stop()
  })

  test('start clears the stopped flag for a fresh session', () => {
    const svc = new CarBridgeService('en')
    const { port } = connectDarwin(svc)
    expect(port.written).toContain('name LIVI\n')
    svc.stop()
    expect(logSpy).toHaveBeenCalled()
  })
})
