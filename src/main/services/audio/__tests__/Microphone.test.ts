import { EventEmitter } from 'events'
import os from 'os'
import { spawn, execSync } from 'child_process'
import Microphone from '@main/services/audio/Microphone'

jest.mock('child_process', () => ({
  spawn: jest.fn(),
  execSync: jest.fn()
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
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('resolveSysdefaultDevice linux parses sysdefault card', () => {
    jest.spyOn(os, 'platform').mockReturnValue('linux')
    ;(execSync as jest.Mock).mockReturnValue('sysdefault:CARD=PCH\n  HDA Intel PCH\n')

    expect(Microphone.resolveSysdefaultDevice()).toBe('plughw:CARD=PCH,DEV=0')
  })

  test('resolveSysdefaultDevice linux fallback on failure', () => {
    jest.spyOn(os, 'platform').mockReturnValue('linux')
    ;(execSync as jest.Mock).mockImplementation(() => {
      throw new Error('fail')
    })

    expect(Microphone.resolveSysdefaultDevice()).toBe('plughw:0,0')
  })

  test('getSysdefaultPrettyName linux returns description line', () => {
    jest.spyOn(os, 'platform').mockReturnValue('linux')
    ;(execSync as jest.Mock).mockReturnValue('null\nsysdefault:CARD=PCH\n  HDA Intel PCH, ALC\n')

    expect(Microphone.getSysdefaultPrettyName()).toBe('HDA Intel PCH, ALC')
  })

  test('start on linux spawns arecord and forwards stdout data event', () => {
    jest.spyOn(os, 'platform').mockReturnValue('linux')
    ;(execSync as jest.Mock).mockReturnValue('sysdefault:CARD=PCH\n')

    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    const onData = jest.fn()
    mic.on('data', onData)

    mic.start()

    expect(spawn).toHaveBeenCalledWith(
      'arecord',
      expect.arrayContaining(['-D', 'plughw:CARD=PCH,DEV=0', '-f', 'S16_LE']),
      expect.any(Object)
    )

    const chunk = Buffer.from([1, 2, 3])
    proc.stdout.emit('data', chunk)
    expect(onData).toHaveBeenCalledWith(chunk)
  })

  test('stop kills active process', () => {
    jest.spyOn(os, 'platform').mockReturnValue('linux')
    ;(execSync as jest.Mock).mockReturnValue('sysdefault:CARD=PCH\n')

    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const mic = new Microphone()
    mic.start()
    mic.stop()

    expect(proc.kill).toHaveBeenCalledTimes(1)
  })

  test('resolveSysdefaultDevice win32 picks Alternative name when available', () => {
    jest.spyOn(os, 'platform').mockReturnValue('win32')
    ;(execSync as jest.Mock).mockReturnValue(
      '[dshow @ ...]  "Microphone (USB)" (audio)\n[dshow @ ...]     Alternative name "@device_cm_123"\n'
    )

    expect(Microphone.resolveSysdefaultDevice('C:\\ffmpeg.exe')).toBe('@device_cm_123')
  })
})
