import { EventEmitter } from 'events'
import os from 'os'
import fs from 'fs'
import { spawn, execSync } from 'child_process'
import { AudioOutput } from '@main/services/audio/AudioOutput'

jest.mock('child_process', () => ({
  spawn: jest.fn(),
  execSync: jest.fn()
}))

jest.mock('fs', () => ({
  existsSync: jest.fn()
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
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('start on linux spawns pw-play and write sends pcm to stdin', () => {
    jest.spyOn(os, 'platform').mockReturnValue('linux')

    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()
    out.write(new Int16Array([1, 2, 3, 4]))

    expect(spawn).toHaveBeenCalledWith(
      'pw-play',
      expect.arrayContaining(['--raw', '--rate', '48000', '--channels', '2', '-']),
      expect.any(Object)
    )
    expect(proc.stdin.write).toHaveBeenCalledTimes(1)
  })

  test('stop ends stdin and kills process', () => {
    jest.spyOn(os, 'platform').mockReturnValue('linux')

    const proc = makeProc()
    ;(spawn as jest.Mock).mockReturnValue(proc)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()
    out.stop()

    expect(proc.stdin.end).toHaveBeenCalledTimes(1)
    expect(proc.kill).toHaveBeenCalledTimes(1)
  })

  test('darwin start without rec binary logs error and does not spawn', () => {
    jest.spyOn(os, 'platform').mockReturnValue('darwin')
    ;(fs.existsSync as jest.Mock).mockReturnValue(false)
    ;(execSync as jest.Mock).mockImplementation(() => {
      throw new Error('not found')
    })

    const errSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const out = new AudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()

    expect(spawn).not.toHaveBeenCalled()
    expect(errSpy).toHaveBeenCalled()

    errSpy.mockRestore()
  })

  test('private ffplayChannelLayout maps known layouts and throws for unknown', () => {
    const cls = AudioOutput as any

    expect(cls.ffplayChannelLayout(1)).toBe('mono')
    expect(cls.ffplayChannelLayout(2)).toBe('stereo')
    expect(cls.ffplayChannelLayout(6)).toBe('5.1')
    expect(() => cls.ffplayChannelLayout(3)).toThrow('Unsupported channel count')
  })
})
