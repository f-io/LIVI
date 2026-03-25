import { EventEmitter } from 'events'
import { spawn } from 'child_process'
import fs from 'fs'
import Microphone from '@main/services/audio/Microphone'
import { app } from 'electron'

jest.mock('child_process', () => ({
  spawn: jest.fn()
}))

jest.mock('fs', () => ({
  existsSync: jest.fn()
}))

jest.mock('electron', () => ({
  app: {
    isPackaged: false,
    getAppPath: jest.fn(() => '/mock/app')
  }
}))

jest.mock('@shared/types', () => ({
  decodeTypeMap: {
    3: { frequency: 8000, channel: 1, bitDepth: 16, format: 's16le' },
    5: { frequency: 16000, channel: 1, bitDepth: 16, format: 's16le' },
    7: { frequency: 48000, channel: 2, bitDepth: 16, format: 'pcm' }
  }
}))

type MockProc = EventEmitter & {
  stdout: EventEmitter
  stderr: EventEmitter
  kill: jest.Mock
}

function makeProc(): MockProc {
  const p = new EventEmitter() as MockProc
  p.stdout = new EventEmitter()
  p.stderr = new EventEmitter()
  p.kill = jest.fn()
  return p
}

describe('Microphone', () => {
  const originalPlatform = process.platform
  const originalArch = process.arch

  beforeEach(() => {
    jest.clearAllMocks()

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'arm64',
      configurable: true
    })
    ;(app as any).isPackaged = false
    ;(app.getAppPath as jest.Mock).mockReturnValue('/mock/app')
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) => {
      return String(p).includes('/mock/app/assets/gstreamer/macos-arm64')
    })
  })

  afterAll(() => {
    Object.defineProperty(process, 'platform', {
      value: originalPlatform,
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: originalArch,
      configurable: true
    })
  })

  test('getSysdefaultPrettyName returns system default', () => {
    expect(Microphone.getSysdefaultPrettyName()).toBe('system default')
  })

  test('start spawns gst-launch on darwin and forwards stdout data', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    const onData = jest.fn()
    mic.on('data', onData)

    mic.start(5)

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/macos-arm64/bin/gst-launch-1.0',
      expect.arrayContaining([
        '-q',
        'osxaudiosrc',
        'queue',
        'max-size-time=20000000',
        'audioconvert',
        'audioresample',
        'audio/x-raw,format=S16LE,rate=16000,channels=1',
        'fdsink',
        'fd=1'
      ]),
      expect.any(Object)
    )

    const chunk = Buffer.from([1, 2, 3, 4])
    proc.stdout.emit('data', chunk)

    expect(onData).toHaveBeenCalledWith(chunk)
  })

  test('start uses decodeType-driven format', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    mic.start(3)

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/macos-arm64/bin/gst-launch-1.0',
      expect.arrayContaining(['audio/x-raw,format=S16LE,rate=8000,channels=1']),
      expect.any(Object)
    )
  })

  test('start falls back to default format when decode type is unknown', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    mic.start(999)

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/macos-arm64/bin/gst-launch-1.0',
      expect.arrayContaining(['audio/x-raw,format=S16LE,rate=16000,channels=1']),
      expect.any(Object)
    )
  })

  test('stop kills active process', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    mic.start(5)
    mic.stop()

    expect(proc.kill).toHaveBeenCalledTimes(1)
  })

  test('stop does nothing when no process exists', () => {
    const mic = new Microphone()

    expect(() => mic.stop()).not.toThrow()
  })

  test('start does not spawn when gstreamer root is missing', () => {
    ;(fs.existsSync as jest.Mock).mockReturnValue(false)
    const errSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const mic = new Microphone()
    mic.start(5)

    expect(spawn).not.toHaveBeenCalled()
    expect(errSpy).toHaveBeenCalledWith('[Microphone] Bundled GStreamer not found')
  })

  test('start does not spawn on unsupported platform', () => {
    Object.defineProperty(process, 'platform', {
      value: 'freebsd',
      configurable: true
    })
    const errSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const mic = new Microphone()
    mic.start(5)

    expect(spawn).not.toHaveBeenCalled()
    expect(errSpy).toHaveBeenCalledWith('[Microphone] Unsupported platform')
  })

  test('isCapturing reflects process state', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    expect(mic.isCapturing()).toBe(false)

    mic.start(5)
    expect(mic.isCapturing()).toBe(true)

    mic.stop()
    expect(mic.isCapturing()).toBe(false)
  })

  test('process error cleans up capture state', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)
    const errSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const mic = new Microphone()
    mic.start(5)

    expect(mic.isCapturing()).toBe(true)

    proc.emit('error', new Error('mic failed'))

    expect(errSpy).toHaveBeenCalledWith('[Microphone] process error:', expect.any(Error))
    expect(mic.isCapturing()).toBe(false)
  })

  test('process close cleans up capture state', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    mic.start(5)

    expect(mic.isCapturing()).toBe(true)

    proc.emit('close', 0, null)

    expect(mic.isCapturing()).toBe(false)
  })

  test('cleanup ignores stale process objects', () => {
    const mic = new Microphone() as any
    const current = makeProc()
    const stale = makeProc()

    mic.process = current
    mic.bytesRead = 100
    mic.chunkSeq = 2

    mic.cleanup(stale)

    expect(mic.process).toBe(current)
    expect(mic.bytesRead).toBe(100)
    expect(mic.chunkSeq).toBe(2)
  })

  test('resolveFormat returns mapped format and fallback default', () => {
    const cls = Microphone as any

    expect(cls.resolveFormat(3)).toEqual({
      frequency: 8000,
      channel: 1,
      bitDepth: 16,
      format: 's16le'
    })

    expect(cls.resolveFormat(12345)).toEqual({
      frequency: 16000,
      channel: 1,
      bitDepth: 16,
      format: 's16le'
    })
  })

  test('toGstRawFormat maps s16le variants and uppercases unknown formats', () => {
    const cls = Microphone as any

    expect(cls.toGstRawFormat({ format: 's16le' })).toBe('S16LE')
    expect(cls.toGstRawFormat({ format: 's16_le' })).toBe('S16LE')
    expect(cls.toGstRawFormat({ format: 'pcm' })).toBe('PCM')
  })

  test('resolveGStreamerRoot returns null when platform/arch combo is unsupported', () => {
    const cls = Microphone as any

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'x64',
      configurable: true
    })

    expect(cls.resolveGStreamerRoot()).toBeNull()
  })

  test('resolveGStreamerRoot returns linux-x64 asset path in dev mode', () => {
    const cls = Microphone as any

    Object.defineProperty(process, 'platform', {
      value: 'linux',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'x64',
      configurable: true
    })
    ;(app as any).isPackaged = false
    ;(app.getAppPath as jest.Mock).mockReturnValue('/mock/app')
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/linux-x64')
    )

    expect(cls.resolveGStreamerRoot()).toBe('/mock/app/assets/gstreamer/linux-x64')
  })

  test('resolveGStreamerRoot returns windows-x64 asset path in dev mode', () => {
    const cls = Microphone as any

    Object.defineProperty(process, 'platform', {
      value: 'win32',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'x64',
      configurable: true
    })
    ;(app as any).isPackaged = false
    ;(app.getAppPath as jest.Mock).mockReturnValue('/mock/app')
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/windows-x64')
    )

    expect(cls.resolveGStreamerRoot()).toBe('/mock/app/assets/gstreamer/windows-x64')
  })

  test('resolveGStreamerRoot uses resourcesPath when app is packaged', () => {
    const cls = Microphone as any

    Object.defineProperty(process, 'platform', {
      value: 'linux',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'x64',
      configurable: true
    })
    Object.defineProperty(process, 'resourcesPath', {
      value: '/mock/resources',
      configurable: true
    })
    ;(app as any).isPackaged = true
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/resources/gstreamer/linux-x64')
    )

    expect(cls.resolveGStreamerRoot()).toBe('/mock/resources/gstreamer/linux-x64')
    ;(app as any).isPackaged = false
  })
})
