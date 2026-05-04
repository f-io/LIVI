import { AudioOutput } from '@main/services/audio/AudioOutput'
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

type MockProc = EventEmitter & {
  stdin: EventEmitter & {
    destroyed: boolean
    write: jest.Mock
    end: jest.Mock
  }
  stderr: EventEmitter
  kill: jest.Mock
}

function makeProc(): MockProc {
  const stdin = new EventEmitter() as MockProc['stdin']
  stdin.destroyed = false
  stdin.write = jest.fn(() => true)
  stdin.end = jest.fn()

  const stderr = new EventEmitter()

  const p = new EventEmitter() as MockProc
  p.stdin = stdin
  p.stderr = stderr
  p.kill = jest.fn()

  return p
}

describe('AudioOutput', () => {
  const originalPlatform = process.platform
  const originalArch = process.arch

  beforeEach(() => {
    jest.clearAllMocks()

    Object.defineProperty(process, 'platform', {
      value: 'darwin'
    })
    Object.defineProperty(process, 'arch', {
      value: 'arm64'
    })
    ;(app.getAppPath as jest.Mock).mockReturnValue('/mock/app')
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) => {
      return String(p).includes('/mock/app/assets/gstreamer/macos-arm64')
    })
  })

  afterAll(() => {
    Object.defineProperty(process, 'platform', {
      value: originalPlatform
    })
    Object.defineProperty(process, 'arch', {
      value: originalArch
    })
  })

  test('start on darwin spawns gst-launch and write sends pcm to stdin', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()
    out.write(new Int16Array([1, 2, 3, 4]))

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/macos-arm64/bin/gst-launch-1.0',
      expect.arrayContaining([
        'fdsrc',
        'fd=0',
        'rawaudioparse',
        'format=pcm',
        'pcm-format=s16le',
        'sample-rate=48000',
        'num-channels=2',
        'audio/x-raw,format=S16LE,rate=48000,channels=2',
        'osxaudiosink'
      ]),
      expect.any(Object)
    )

    expect(proc.stdin.write).toHaveBeenCalledTimes(1)
  })

  test('realtime mode uses small non-leaky queues and sync=false sink args', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 16000, channels: 1, mode: 'realtime' })
    out.start()

    const [, args] = (spawn as jest.Mock).mock.calls[0]

    // Realtime queues are small (40 ms input, 20 ms output) but must NOT be
    // leaky — leaking the oldest buffers would drop the start of every short
    // announcement during pipeline + pulsesink startup latency.
    expect(args).toEqual(
      expect.arrayContaining([
        'max-size-time=40000000',
        'max-size-time=20000000',
        'sample-rate=16000',
        'num-channels=1',
        'audio/x-raw,format=S16LE,rate=48000,channels=2',
        'sync=false'
      ])
    )
    expect(args).not.toContain('leaky=downstream')
    expect(args).not.toContain('leaky=upstream')
  })

  test('stop closes stdin synchronously and SIGTERMs as a fallback after grace period', () => {
    jest.useFakeTimers()
    try {
      const proc = makeProc()
      ;(spawn as jest.Mock).mockReturnValue(proc)

      const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
      out.start()
      out.stop()

      // EOS sent immediately so pulsesink can drain its tail …
      expect(proc.stdin.end).toHaveBeenCalledTimes(1)
      // … but the kill is deferred so the drain can actually happen.
      expect(proc.kill).not.toHaveBeenCalled()

      // After the grace period, if the process hasn't exited on its own, we
      // fall back to SIGTERM.
      jest.advanceTimersByTime(500)
      expect(proc.kill).toHaveBeenCalledTimes(1)
    } finally {
      jest.useRealTimers()
    }
  })

  test('stop fallback SIGTERM is skipped if the process exited cleanly first', () => {
    jest.useFakeTimers()
    try {
      const proc = makeProc()
      ;(spawn as jest.Mock).mockReturnValue(proc)

      const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
      out.start()
      out.stop()

      // Simulate the process draining and exiting before the grace timer fires.
      proc.emit('close', 0, null)

      jest.advanceTimersByTime(500)
      expect(proc.kill).not.toHaveBeenCalled()
    } finally {
      jest.useRealTimers()
    }
  })

  test('stop fallback SIGTERM is skipped if a new process replaced the old one', () => {
    jest.useFakeTimers()
    try {
      const proc1 = makeProc()
      const proc2 = makeProc()
      ;(spawn as jest.Mock).mockReturnValueOnce(proc1).mockReturnValueOnce(proc2)

      const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
      out.start()
      out.stop()
      // proc1 is now draining; user immediately starts a new playback.
      out.start()

      // proc1 was force-killed by start()'s killImmediate.
      expect(proc1.kill).toHaveBeenCalledTimes(1)
      const proc1KillCallsBefore = proc1.kill.mock.calls.length

      // The deferred fallback fires later but must be a no-op now.
      jest.advanceTimersByTime(500)
      expect(proc1.kill).toHaveBeenCalledTimes(proc1KillCallsBefore)
      expect(proc2.kill).not.toHaveBeenCalled()
    } finally {
      jest.useRealTimers()
    }
  })

  test('darwin start without bundled gstreamer logs error and does not spawn', () => {
    ;(app.getAppPath as jest.Mock).mockReturnValue('/mock/app')
    ;(fs.existsSync as jest.Mock).mockReturnValue(false)

    const errSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()

    expect(spawn).not.toHaveBeenCalled()
    expect(errSpy).toHaveBeenCalledWith('[AudioOutput] Bundled GStreamer not found')

    errSpy.mockRestore()
  })

  test('write does nothing when process is not started', () => {
    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })

    expect(() => out.write(new Int16Array([1, 2, 3, 4]))).not.toThrow()
    expect(spawn).not.toHaveBeenCalled()
  })

  test('inferMode chooses realtime for mono low-rate and music otherwise', () => {
    const cls = AudioOutput as any

    expect(cls.inferMode(16000, 1)).toBe('realtime')
    expect(cls.inferMode(24000, 2)).toBe('realtime')
    expect(cls.inferMode(48000, 2)).toBe('music')
  })

  test('constructor infers mode automatically when mode is omitted', () => {
    const realtime = new AudioOutput({ sampleRate: 16000, channels: 1 }) as any
    const music = new AudioOutput({ sampleRate: 48000, channels: 2 }) as any

    expect(realtime.mode).toBe('realtime')
    expect(music.mode).toBe('music')
  })

  test('constructor clamps channels to at least 1', () => {
    const out = new AudioOutput({ sampleRate: 48000, channels: 0, mode: 'music' }) as any

    expect(out.channels).toBe(1)
  })

  test('start stops previous process before spawning a new one', () => {
    const proc1 = makeProc()
    const proc2 = makeProc()
    ;(spawn as jest.Mock).mockReturnValueOnce(proc1).mockReturnValueOnce(proc2)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()
    out.start()

    expect(proc1.stdin.end).toHaveBeenCalledTimes(1)
    expect(proc1.kill).toHaveBeenCalledTimes(1)
    expect(spawn).toHaveBeenCalledTimes(2)
  })

  test('start on unsupported platform logs error and does not spawn', () => {
    Object.defineProperty(process, 'platform', { value: 'freebsd' })
    const errSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()

    expect(spawn).not.toHaveBeenCalled()
    expect(errSpy).toHaveBeenCalledWith('[AudioOutput] Unsupported platform')

    errSpy.mockRestore()
  })

  test('start on linux uses pulsesink and linux gstreamer env', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    Object.defineProperty(process, 'arch', { value: 'x64' })
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/linux-x64')
    )

    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/linux-x64/bin/gst-launch-1.0',
      expect.arrayContaining(['pulsesink']),
      expect.objectContaining({
        env: expect.objectContaining({
          LD_LIBRARY_PATH: '/mock/app/assets/gstreamer/linux-x64/lib',
          GST_PLUGIN_PATH: '/mock/app/assets/gstreamer/linux-x64/lib/gstreamer-1.0'
        }),
        shell: false
      })
    )
  })

  test('start on win32 uses wasapisink, exe binary and omits audio/x-raw caps', () => {
    Object.defineProperty(process, 'platform', { value: 'win32' })
    Object.defineProperty(process, 'arch', { value: 'x64' })
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) =>
      String(p).includes('/mock/app/assets/gstreamer/windows-x64')
    )

    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()

    const [cmd, args, opts] = (spawn as jest.Mock).mock.calls[0]

    expect(cmd).toBe('/mock/app/assets/gstreamer/windows-x64/bin/gst-launch-1.0.exe')
    expect(args).toEqual(expect.arrayContaining(['wasapisink']))
    expect(args).not.toContain('audio/x-raw,format=S16LE,rate=48000,channels=2')
    expect(opts).toEqual(
      expect.objectContaining({
        env: expect.objectContaining({
          PATH: expect.stringContaining('/mock/app/assets/gstreamer/windows-x64/bin'),
          GST_PLUGIN_PATH: '/mock/app/assets/gstreamer/windows-x64/lib/gstreamer-1.0',
          GST_PLUGIN_SCANNER:
            '/mock/app/assets/gstreamer/windows-x64/libexec/gstreamer-1.0/gst-plugin-scanner.exe'
        })
      })
    )
  })

  test('write accepts Buffer chunks', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()
    out.write(Buffer.from([1, 2, 3, 4]))

    expect(proc.stdin.write).toHaveBeenCalledWith(Buffer.from([1, 2, 3, 4]))
  })

  test('write ignores null and undefined chunks', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()
    out.write(undefined)
    out.write(null)

    expect(proc.stdin.write).not.toHaveBeenCalled()
  })

  test('write returns early when stdin is destroyed', () => {
    const proc = makeProc()
    proc.stdin.destroyed = true
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()
    out.write(new Int16Array([1, 2]))

    expect(proc.stdin.write).not.toHaveBeenCalled()
  })

  test('flushQueue keeps remaining buffers queued on backpressure and drain flushes them', () => {
    const proc = makeProc()
    proc.stdin.write.mockReturnValueOnce(false).mockReturnValueOnce(true).mockReturnValueOnce(true)
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' }) as any
    out.start()

    out.write(Buffer.from([1, 2]))
    out.write(Buffer.from([3, 4]))

    expect(proc.stdin.write).toHaveBeenCalledTimes(1)
    expect(out.queue).toHaveLength(1)
    expect(out.writing).toBe(true)

    proc.stdin.emit('drain')

    expect(proc.stdin.write).toHaveBeenCalledTimes(2)
    expect(out.queue).toHaveLength(0)
    expect(out.writing).toBe(false)
  })

  test('flushQueue clears queue when process disappears', () => {
    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' }) as any
    out.queue = [Buffer.from([1, 2])]
    out.process = null
    out.writing = true

    out.flushQueue()

    expect(out.queue).toEqual([])
    expect(out.writing).toBe(false)
  })

  test('stdin error listener does not throw', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()

    expect(() => proc.stdin.emit('error', new Error('stdin failed'))).not.toThrow()
  })

  test('process error triggers cleanup', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' }) as any
    out.start()
    out.queue = [Buffer.from([1])]
    out.writing = true

    proc.emit('error', new Error('proc failed'))

    expect(out.process).toBeNull()
    expect(out.queue).toEqual([])
    expect(out.writing).toBe(false)
  })

  test('process close triggers cleanup', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' }) as any
    out.start()
    out.queue = [Buffer.from([1])]
    out.writing = true

    proc.emit('close', 0, null)

    expect(out.process).toBeNull()
    expect(out.queue).toEqual([])
    expect(out.writing).toBe(false)
  })

  test('dispose immediately tears the process down without waiting for drain', () => {
    // dispose() is used during app shutdown. We want a synchronous kill so
    // we don't leak a gst-launch zombie past the parent process exit, and we
    // accept that the tail of any in-flight playback is lost.
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()
    out.dispose()

    expect(proc.stdin.end).toHaveBeenCalledTimes(1)
    expect(proc.kill).toHaveBeenCalledTimes(1)
  })

  test('stop is a no-op when there is no active process', () => {
    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })

    expect(() => out.stop()).not.toThrow()
  })

  test('stop swallows stdin.end and kill errors', () => {
    jest.useFakeTimers()
    try {
      const proc = makeProc()
      proc.stdin.end.mockImplementation(() => {
        throw new Error('end fail')
      })
      proc.kill.mockImplementation(() => {
        throw new Error('kill fail')
      })
      ;(spawn as jest.Mock).mockReturnValue(proc)

      const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })

      out.start()

      expect(() => out.stop()).not.toThrow()
      // The deferred SIGTERM also fires inside the timer callback and must
      // not propagate the kill error either.
      expect(() => jest.advanceTimersByTime(500)).not.toThrow()
    } finally {
      jest.useRealTimers()
    }
  })

  test('resolveGStreamerRoot returns null for unsupported arch/platform combinations', () => {
    const cls = AudioOutput as any

    Object.defineProperty(process, 'platform', { value: 'darwin' })
    Object.defineProperty(process, 'arch', { value: 'x64' })
    expect(cls.resolveGStreamerRoot()).toBeNull()

    Object.defineProperty(process, 'platform', { value: 'linux' })
    Object.defineProperty(process, 'arch', { value: 'ppc64' })
    expect(cls.resolveGStreamerRoot()).toBeNull()

    Object.defineProperty(process, 'platform', { value: 'win32' })
    Object.defineProperty(process, 'arch', { value: 'arm64' })
    expect(cls.resolveGStreamerRoot()).toBeNull()
  })

  test('resolveGStreamerRoot uses resourcesPath when app is packaged', () => {
    const cls = AudioOutput as any

    Object.defineProperty(process, 'platform', { value: 'linux' })
    Object.defineProperty(process, 'arch', { value: 'x64' })
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

  test('constructor normalizes fractional and negative channel counts', () => {
    const a = new AudioOutput({ sampleRate: 48000, channels: 2.9, mode: 'music' }) as any
    const b = new AudioOutput({ sampleRate: 48000, channels: -3, mode: 'music' }) as any

    expect(a.channels).toBe(2)
    expect(b.channels).toBe(1)
  })

  test('write flushes immediately only when not already writing', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' }) as any
    out.start()
    out.writing = true

    out.write(Buffer.from([1, 2, 3, 4]))

    expect(proc.stdin.write).not.toHaveBeenCalled()
    expect(out.queue).toHaveLength(1)
  })

  test('stop does not end stdin when stdin is already destroyed but still SIGTERMs as fallback', () => {
    jest.useFakeTimers()
    try {
      const proc = makeProc()
      proc.stdin.destroyed = true
      ;(spawn as jest.Mock).mockReturnValue(proc)

      const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
      out.start()
      out.stop()

      expect(proc.stdin.end).not.toHaveBeenCalled()

      jest.advanceTimersByTime(500)
      expect(proc.kill).toHaveBeenCalledTimes(1)
    } finally {
      jest.useRealTimers()
    }
  })

  test('resolveGStreamerRoot returns null when supported platform dir does not exist', () => {
    const cls = AudioOutput as any

    Object.defineProperty(process, 'platform', { value: 'linux' })
    Object.defineProperty(process, 'arch', { value: 'x64' })
    ;(app as any).isPackaged = false
    ;(app.getAppPath as jest.Mock).mockReturnValue('/mock/app')
    ;(fs.existsSync as jest.Mock).mockReturnValue(false)

    expect(cls.resolveGStreamerRoot()).toBeNull()
  })

  test('cleanup clears queue, writing flag and process', () => {
    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' }) as any
    out.process = makeProc()
    out.queue = [Buffer.from([1]), Buffer.from([2])]
    out.writing = true

    out.cleanup()

    expect(out.process).toBeNull()
    expect(out.queue).toEqual([])
    expect(out.writing).toBe(false)
  })

  test('linux realtime buildArgs uses pulsesink with sync=false and non-leaky queues', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const out = new AudioOutput({ sampleRate: 16000, channels: 1, mode: 'realtime' }) as any
    const args = out.buildArgs()

    expect(args).toEqual(expect.arrayContaining(['pulsesink', 'sync=false']))
    expect(args).not.toContain('leaky=downstream')
    expect(args).not.toContain('leaky=upstream')
  })

  test('emits debug logs for constructor, spawn, stdin error, drain, stderr, process error and close when DEBUG is enabled', () => {
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

    const { AudioOutput: DebugAudioOutput } =
      require('@main/services/audio/AudioOutput') as typeof import('@main/services/audio/AudioOutput')

    const proc = makeProc()
    freshSpawn.mockReturnValue(proc)

    const debugSpy = jest.spyOn(console, 'debug').mockImplementation(() => undefined)
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => undefined)
    const errorSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const out = new DebugAudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()

    proc.stdin.emit('error', new Error('stdin failed'))
    proc.stdin.emit('drain')
    proc.stderr.emit('data', Buffer.from('gst stderr'))
    proc.emit('error', new Error('proc failed'))
    proc.emit('close', 0, null)

    expect(debugSpy).toHaveBeenCalledWith(
      '[AudioOutput] Init',
      expect.objectContaining({
        sampleRate: 48000,
        channels: 2,
        mode: 'music',
        platform: 'darwin'
      })
    )

    expect(debugSpy).toHaveBeenCalledWith(
      '[AudioOutput] Spawning',
      '/mock/app/assets/gstreamer/macos-arm64/bin/gst-launch-1.0',
      expect.any(String)
    )

    expect(warnSpy).toHaveBeenCalledWith('[AudioOutput] stdin error:', 'stdin failed')

    expect(debugSpy).toHaveBeenCalledWith(
      '[AudioOutput] stdin drain',
      expect.objectContaining({
        mode: 'music',
        queueLength: expect.any(Number),
        bytesWritten: expect.any(Number),
        ts: expect.any(Number)
      })
    )

    expect(warnSpy).toHaveBeenCalledWith('[AudioOutput] STDERR:', 'gst stderr')

    expect(errorSpy).toHaveBeenCalledWith('[AudioOutput] process error:', expect.any(Error))

    expect(debugSpy).toHaveBeenCalledWith(
      '[AudioOutput] process exited',
      expect.objectContaining({
        code: 0,
        signal: null,
        mode: 'music',
        bytesWritten: expect.any(Number),
        ts: expect.any(Number)
      })
    )
  })

  test('logs write queued on first and hundredth write and warns on stdin backpressure when DEBUG is enabled', () => {
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

    const { AudioOutput: DebugAudioOutput } =
      require('@main/services/audio/AudioOutput') as typeof import('@main/services/audio/AudioOutput')

    const proc = makeProc()
    proc.stdin.write
      .mockReturnValueOnce(false) // first write => backpressure
      .mockReturnValue(true)

    freshSpawn.mockReturnValue(proc)

    const debugSpy = jest.spyOn(console, 'debug').mockImplementation(() => undefined)
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => undefined)

    const out = new DebugAudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()

    out.write(Buffer.from([1, 2, 3, 4]))

    expect(debugSpy).toHaveBeenCalledWith(
      '[AudioOutput] write queued',
      expect.objectContaining({
        seq: 1,
        chunkBytes: 4,
        queueLength: 1,
        ts: expect.any(Number)
      })
    )

    expect(warnSpy).toHaveBeenCalledWith(
      '[AudioOutput] stdin backpressure',
      expect.objectContaining({
        mode: 'music',
        queueLength: 0,
        bytesWritten: 4,
        ts: expect.any(Number)
      })
    )

    debugSpy.mockClear()

    for (let i = 0; i < 99; i += 1) {
      out.write(Buffer.from([9]))
    }

    expect(debugSpy).toHaveBeenCalledWith(
      '[AudioOutput] write queued',
      expect.objectContaining({
        seq: 100,
        chunkBytes: 1,
        ts: expect.any(Number)
      })
    )
  })

  test('warns when stdin.end and kill throw while stopping in DEBUG mode', () => {
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

    const { AudioOutput: DebugAudioOutput } =
      require('@main/services/audio/AudioOutput') as typeof import('@main/services/audio/AudioOutput')

    const proc = makeProc()
    proc.stdin.end.mockImplementation(() => {
      throw new Error('end fail')
    })
    proc.kill.mockImplementation(() => {
      throw new Error('kill fail')
    })

    freshSpawn.mockReturnValue(proc)

    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => undefined)

    jest.useFakeTimers()
    try {
      const out = new DebugAudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
      out.start()
      out.stop()

      // stdin.end throws synchronously inside stop()
      expect(warnSpy).toHaveBeenCalledWith('[AudioOutput] failed to end stdin:', expect.any(Error))

      // kill throws inside the deferred SIGTERM callback
      jest.advanceTimersByTime(500)
      expect(warnSpy).toHaveBeenCalledWith('[AudioOutput] failed to kill process:', expect.any(Error))
    } finally {
      jest.useRealTimers()
    }
  })
})
