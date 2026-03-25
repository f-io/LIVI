import { EventEmitter } from 'events'
import fs from 'fs'
import { spawn } from 'child_process'
import { AudioOutput } from '@main/services/audio/AudioOutput'
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

  test('realtime mode adds leaky queues and sync=false sink args', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 16000, channels: 1, mode: 'realtime' })
    out.start()

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/macos-arm64/bin/gst-launch-1.0',
      expect.arrayContaining([
        'leaky=downstream',
        'sample-rate=16000',
        'num-channels=1',
        'audio/x-raw,format=S16LE,rate=48000,channels=2',
        'sync=false'
      ]),
      expect.any(Object)
    )
  })

  test('stop ends stdin and kills process', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()
    out.stop()

    expect(proc.stdin.end).toHaveBeenCalledTimes(1)
    expect(proc.kill).toHaveBeenCalledTimes(1)
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

  test('dispose delegates to stop', () => {
    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    const stopSpy = jest.spyOn(out, 'stop').mockImplementation(() => undefined)

    out.dispose()

    expect(stopSpy).toHaveBeenCalledTimes(1)
  })

  test('stop is a no-op when there is no active process', () => {
    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })

    expect(() => out.stop()).not.toThrow()
  })

  test('stop swallows stdin.end and kill errors', () => {
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
})
