import Microphone from '@main/services/audio/Microphone'
import { spawn } from 'child_process'
import { app } from 'electron'
import { EventEmitter } from 'events'
import fs from 'fs'

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

  test('start spawns gst-launch on linux with pulsesrc and linux env', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    Object.defineProperty(process, 'platform', {
      value: 'linux',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'x64',
      configurable: true
    })
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/linux-x64')
    )

    const mic = new Microphone()
    mic.start(5)

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/linux-x64/bin/gst-launch-1.0',
      expect.arrayContaining([
        '-q',
        'pulsesrc',
        'audio/x-raw,format=S16LE,rate=16000,channels=1',
        'fdsink',
        'fd=1'
      ]),
      expect.objectContaining({
        env: expect.objectContaining({
          LD_LIBRARY_PATH: '/mock/app/assets/gstreamer/linux-x64/lib',
          GST_PLUGIN_SYSTEM_PATH: '',
          GST_PLUGIN_PATH: '/mock/app/assets/gstreamer/linux-x64/lib/gstreamer-1.0',
          GST_PLUGIN_SCANNER:
            '/mock/app/assets/gstreamer/linux-x64/libexec/gstreamer-1.0/gst-plugin-scanner'
        }),
        shell: false
      })
    )
  })

  test('start spawns gst-launch on win32 with wasapisrc and windows env', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    Object.defineProperty(process, 'platform', {
      value: 'win32',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'x64',
      configurable: true
    })
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/windows-x64')
    )

    const mic = new Microphone()
    mic.start(7)

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/windows-x64/bin/gst-launch-1.0.exe',
      expect.arrayContaining([
        '-q',
        'wasapisrc',
        'audio/x-raw,format=PCM,rate=48000,channels=2',
        'fdsink',
        'fd=1'
      ]),
      expect.objectContaining({
        env: expect.objectContaining({
          PATH: expect.stringContaining('/mock/app/assets/gstreamer/windows-x64/bin;'),
          GST_PLUGIN_SYSTEM_PATH: '',
          GST_PLUGIN_PATH: '/mock/app/assets/gstreamer/windows-x64/lib/gstreamer-1.0',
          GST_PLUGIN_SCANNER:
            '/mock/app/assets/gstreamer/windows-x64/libexec/gstreamer-1.0/gst-plugin-scanner.exe'
        }),
        shell: false
      })
    )
  })

  test('start handles falsy spawn result and cleans up', () => {
    const errSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)
    ;(spawn as jest.Mock).mockReturnValue(null)

    const mic = new Microphone() as any
    mic.bytesRead = 77
    mic.chunkSeq = 8

    mic.start(5)

    expect(errSpy).toHaveBeenCalledWith('[Microphone] Failed to spawn recorder process')
    expect(mic.isCapturing()).toBe(false)
    expect(mic.bytesRead).toBe(0)
    expect(mic.chunkSeq).toBe(0)
  })

  test('stderr handler ignores empty output', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => undefined)

    const mic = new Microphone()
    mic.start(5)

    proc.stderr.emit('data', Buffer.from('   \n'))

    expect(warnSpy).not.toHaveBeenCalled()
  })

  test('stop swallows kill errors', () => {
    const proc = makeProc()
    proc.kill.mockImplementation(() => {
      throw new Error('kill failed')
    })
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()

    mic.start(5)

    expect(() => mic.stop()).not.toThrow()
    expect(mic.isCapturing()).toBe(false)
  })

  test('cleanup without process argument resets internal state', () => {
    const mic = new Microphone() as any
    const proc = makeProc()

    mic.process = proc
    mic.bytesRead = 55
    mic.chunkSeq = 3

    mic.cleanup()

    expect(mic.process).toBeNull()
    expect(mic.bytesRead).toBe(0)
    expect(mic.chunkSeq).toBe(0)
  })

  test('toGstRawFormat falls back to S16LE when format is missing', () => {
    const cls = Microphone as any

    expect(cls.toGstRawFormat({})).toBe('S16LE')
  })

  test('logs debug messages for constructor, start, stderr, stop and close when DEBUG is enabled', () => {
    jest.resetModules()

    jest.doMock('@main/constants', () => ({
      DEBUG: true
    }))

    const { spawn: freshSpawn } = require('child_process') as { spawn: jest.Mock }
    const freshFs = require('fs') as { existsSync: jest.Mock }
    const { app: freshApp } = require('electron') as {
      app: { isPackaged: boolean; getAppPath: jest.Mock }
    }

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'arm64',
      configurable: true
    })

    freshApp.isPackaged = false
    freshApp.getAppPath.mockReturnValue('/mock/app')
    freshFs.existsSync.mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/macos-arm64')
    )

    const DebugMicrophone = require('@main/services/audio/Microphone')
      .default as typeof import('@main/services/audio/Microphone').default

    const proc = makeProc()
    freshSpawn.mockReturnValue(proc)

    const debugSpy = jest.spyOn(console, 'debug').mockImplementation(() => undefined)
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => undefined)

    const mic = new DebugMicrophone()
    mic.start(5)

    proc.stderr.emit('data', Buffer.from('gst warning'))
    mic.stop()
    proc.emit('close', 0, null)

    expect(debugSpy).toHaveBeenCalledWith('[Microphone] Init', expect.any(Object))
    expect(debugSpy).toHaveBeenCalledWith(
      '[Microphone] Recording started',
      expect.objectContaining({
        decodeType: 5,
        frequency: 16000,
        channel: 1,
        bitDepth: 16,
        format: 's16le'
      })
    )
    expect(warnSpy).toHaveBeenCalledWith('[Microphone] STDERR:', 'gst warning')
    expect(debugSpy).toHaveBeenCalledWith(
      '[Microphone] Stopping recording',
      expect.objectContaining({ decodeType: 5 })
    )
    expect(debugSpy).toHaveBeenCalledWith(
      '[Microphone] recorder exited',
      expect.objectContaining({ code: 0, signal: null })
    )
  })

  test('logs debug message when stop is called without active process and when kill throws', () => {
    jest.resetModules()

    jest.doMock('@main/constants', () => ({
      DEBUG: true
    }))

    const { spawn: freshSpawn } = require('child_process') as { spawn: jest.Mock }
    const freshFs = require('fs') as { existsSync: jest.Mock }
    const { app: freshApp } = require('electron') as {
      app: { isPackaged: boolean; getAppPath: jest.Mock }
    }

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'arm64',
      configurable: true
    })

    freshApp.isPackaged = false
    freshApp.getAppPath.mockReturnValue('/mock/app')
    freshFs.existsSync.mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/macos-arm64')
    )

    const DebugMicrophone = require('@main/services/audio/Microphone')
      .default as typeof import('@main/services/audio/Microphone').default

    const debugSpy = jest.spyOn(console, 'debug').mockImplementation(() => undefined)
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => undefined)

    const mic = new DebugMicrophone()
    mic.stop()

    expect(debugSpy).toHaveBeenCalledWith('[Microphone] No active process to stop')

    const proc = makeProc()
    proc.kill.mockImplementation(() => {
      throw new Error('kill failed')
    })
    freshSpawn.mockReturnValue(proc)

    mic.start(5)
    mic.stop()

    expect(warnSpy).toHaveBeenCalledWith('[Microphone] Failed to kill process:', expect.any(Error))
  })

  test('cleanup ignores stale process in active instance', () => {
    const mic = new Microphone() as any
    const current = makeProc()
    const stale = makeProc()

    mic.process = current
    mic.bytesRead = 12
    mic.chunkSeq = 34

    mic.cleanup(stale)

    expect(mic.process).toBe(current)
    expect(mic.bytesRead).toBe(12)
    expect(mic.chunkSeq).toBe(34)
  })

  test('toGstRawFormat uses fallback when format is nullish and resolveGStreamerRoot returns null for unsupported linux arch', () => {
    const cls = Microphone as any

    expect(cls.toGstRawFormat({ format: undefined })).toBe('S16LE')

    Object.defineProperty(process, 'platform', {
      value: 'linux',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'arm',
      configurable: true
    })

    expect(cls.resolveGStreamerRoot()).toBeNull()
  })

  test('logs debug chunk message for first stdout chunk when DEBUG is enabled', () => {
    jest.resetModules()

    jest.doMock('@main/constants', () => ({
      DEBUG: true
    }))

    const { spawn: freshSpawn } = require('child_process') as { spawn: jest.Mock }
    const freshFs = require('fs') as { existsSync: jest.Mock }
    const { app: freshApp } = require('electron') as {
      app: { isPackaged: boolean; getAppPath: jest.Mock }
    }

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'arm64',
      configurable: true
    })

    freshApp.isPackaged = false
    freshApp.getAppPath.mockReturnValue('/mock/app')
    freshFs.existsSync.mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/macos-arm64')
    )

    const DebugMicrophone = require('@main/services/audio/Microphone')
      .default as typeof import('@main/services/audio/Microphone').default

    const proc = makeProc()
    freshSpawn.mockReturnValue(proc)

    const debugSpy = jest.spyOn(console, 'debug').mockImplementation(() => undefined)

    const mic = new DebugMicrophone()
    mic.start(5)

    const chunk = Buffer.from([1, 2, 3, 4])
    proc.stdout.emit('data', chunk)

    expect(debugSpy).toHaveBeenCalledWith(
      '[Microphone] chunk received',
      expect.objectContaining({
        decodeType: 5,
        chunkBytes: 4,
        bytesRead: 4,
        seq: 1,
        ts: expect.any(Number)
      })
    )
  })

  test('does not log constructor debug output when DEBUG is false', () => {
    const debugSpy = jest.spyOn(console, 'debug').mockImplementation(() => undefined)

    new Microphone()

    expect(debugSpy).not.toHaveBeenCalled()
  })

  test('does not warn for stderr output when DEBUG is false', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => undefined)

    const mic = new Microphone()
    mic.start(5)

    proc.stderr.emit('data', Buffer.from('gst warning'))

    expect(warnSpy).not.toHaveBeenCalled()
  })

  test('logs debug chunk message again on chunk 100 when DEBUG is enabled', () => {
    jest.resetModules()

    jest.doMock('@main/constants', () => ({
      DEBUG: true
    }))

    const { spawn: freshSpawn } = require('child_process') as { spawn: jest.Mock }
    const freshFs = require('fs') as { existsSync: jest.Mock }
    const { app: freshApp } = require('electron') as {
      app: { isPackaged: boolean; getAppPath: jest.Mock }
    }

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'arm64',
      configurable: true
    })

    freshApp.isPackaged = false
    freshApp.getAppPath.mockReturnValue('/mock/app')
    freshFs.existsSync.mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/macos-arm64')
    )

    const DebugMicrophone = require('@main/services/audio/Microphone')
      .default as typeof import('@main/services/audio/Microphone').default

    const proc = makeProc()
    freshSpawn.mockReturnValue(proc)

    const debugSpy = jest.spyOn(console, 'debug').mockImplementation(() => undefined)

    const mic = new DebugMicrophone()
    mic.start(5)

    for (let i = 0; i < 100; i += 1) {
      proc.stdout.emit('data', Buffer.from([1]))
    }

    expect(debugSpy).toHaveBeenCalledWith(
      '[Microphone] chunk received',
      expect.objectContaining({
        seq: 100,
        chunkBytes: 1,
        bytesRead: 100
      })
    )
  })

  test('cleanup from stale process is ignored after process replacement via second start', () => {
    const first = makeProc()
    const second = makeProc()

    ;(spawn as jest.Mock).mockReturnValueOnce(first).mockReturnValueOnce(second)

    const mic = new Microphone()

    mic.start(5)
    mic.start(5)

    expect(mic.isCapturing()).toBe(true)

    first.emit('close', 0, null)

    expect(mic.isCapturing()).toBe(true)

    second.emit('close', 0, null)

    expect(mic.isCapturing()).toBe(false)
  })

  test('resolveGStreamerRoot returns null for unsupported win32 arch', () => {
    const cls = Microphone as any

    Object.defineProperty(process, 'platform', {
      value: 'win32',
      configurable: true
    })
    Object.defineProperty(process, 'arch', {
      value: 'arm64',
      configurable: true
    })

    expect(cls.resolveGStreamerRoot()).toBeNull()
  })

  test('resolveGStreamerRoot returns null when supported platform path does not exist', () => {
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
    ;(fs.existsSync as jest.Mock).mockReturnValue(false)

    expect(cls.resolveGStreamerRoot()).toBeNull()
  })
})
