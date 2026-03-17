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

  beforeEach(() => {
    jest.clearAllMocks()

    Object.defineProperty(process, 'platform', {
      value: 'darwin'
    })
    ;(app.getAppPath as jest.Mock).mockReturnValue('/mock/app')
    ;(fs.existsSync as jest.Mock).mockImplementation((p: fs.PathLike) => {
      return String(p).includes('/mock/app/assets/gstreamer/darwin')
    })
  })

  afterAll(() => {
    Object.defineProperty(process, 'platform', {
      value: originalPlatform
    })
  })

  test('start on darwin spawns gst-launch and write sends pcm to stdin', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2, mode: 'music' })
    out.start()
    out.write(new Int16Array([1, 2, 3, 4]))

    expect(spawn).toHaveBeenCalledWith(
      '/mock/app/assets/gstreamer/darwin/bin/gst-launch-1.0',
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
      '/mock/app/assets/gstreamer/darwin/bin/gst-launch-1.0',
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
})
