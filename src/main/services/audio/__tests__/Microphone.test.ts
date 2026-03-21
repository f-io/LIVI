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
    getAppPath: jest.fn()
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

  test('stop kills active process', () => {
    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    mic.start(5)
    mic.stop()

    expect(proc.kill).toHaveBeenCalledTimes(1)
  })

  test('start does not spawn when gstreamer root is missing', () => {
    ;(fs.existsSync as jest.Mock).mockReturnValue(false)

    const mic = new Microphone()
    mic.start(5)

    expect(spawn).not.toHaveBeenCalled()
  })
})
