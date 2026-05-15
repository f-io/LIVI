import { AudioCommand } from '@shared/types/ProjectionEnums'
import fs from 'fs'
import os from 'os'
import path from 'path'

jest.mock('electron', () => ({
  app: { getPath: jest.fn(() => os.tmpdir()) }
}))

import { STATUS_VERSION, StatusFileWriter } from '../StatusFileWriter'

function tmpFile(): string {
  return path.join(
    os.tmpdir(),
    `livi-status-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2)}.json`
  )
}

function readStatus(file: string): { version: number; timestamp: string; payload: unknown } {
  return JSON.parse(fs.readFileSync(file, 'utf8'))
}

beforeEach(() => {
  jest.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('StatusFileWriter — initial write', () => {
  test('writes the INITIAL state on construction', () => {
    const file = tmpFile()
    new StatusFileWriter(file, { debounceMs: 0 })
    const got = readStatus(file)
    expect(got.version).toBe(STATUS_VERSION)
    expect((got.payload as { projection: { active: unknown } }).projection.active).toBeNull()
    expect((got.payload as { phone: { active: boolean } }).phone.active).toBe(false)
    fs.unlinkSync(file)
  })

  test('writeInitial=false skips the eager flush', () => {
    const file = tmpFile()
    new StatusFileWriter(file, { writeInitial: false })
    expect(fs.existsSync(file)).toBe(false)
  })
})

describe('StatusFileWriter — setters mutate state and write', () => {
  test('setProjection updates active + phoneType', () => {
    const file = tmpFile()
    const w = new StatusFileWriter(file, { debounceMs: 0 })
    w.setProjection('aa', 'AndroidAuto')
    w.flush()
    const s = readStatus(file).payload as {
      projection: { active: string; phoneType: string; streaming: boolean }
    }
    expect(s.projection.active).toBe('aa')
    expect(s.projection.phoneType).toBe('AndroidAuto')
    expect(s.projection.streaming).toBe(false)
    fs.unlinkSync(file)
  })

  test('setStreaming preserves the rest of the projection block', () => {
    const file = tmpFile()
    const w = new StatusFileWriter(file, { debounceMs: 0 })
    w.setProjection('dongle', 'CarPlay')
    w.setStreaming(true)
    w.flush()
    const s = readStatus(file).payload as {
      projection: { active: string; phoneType: string; streaming: boolean }
    }
    expect(s.projection).toEqual({ active: 'dongle', phoneType: 'CarPlay', streaming: true })
    fs.unlinkSync(file)
  })

  test('setUsbState writes the USB block', () => {
    const file = tmpFile()
    const w = new StatusFileWriter(file, { debounceMs: 0 })
    w.setUsbState(true, false)
    w.flush()
    const s = readStatus(file).payload as {
      usb: { phoneConnected: boolean; dongleConnected: boolean }
    }
    expect(s.usb).toEqual({ phoneConnected: true, dongleConnected: false })
    fs.unlinkSync(file)
  })

  test('setAudio scopes by channel', () => {
    const file = tmpFile()
    const w = new StatusFileWriter(file, { debounceMs: 0 })
    w.setAudio('media', true)
    w.setAudio('system', true)
    w.flush()
    const s = readStatus(file).payload as {
      audio: {
        media: { playing: boolean }
        speech: { playing: boolean }
        system: { playing: boolean }
      }
    }
    expect(s.audio.media.playing).toBe(true)
    expect(s.audio.speech.playing).toBe(false)
    expect(s.audio.system.playing).toBe(true)
    fs.unlinkSync(file)
  })
})

describe('StatusFileWriter — applyAudioCommand mapping', () => {
  test.each([
    [AudioCommand.AudioMediaStart, ['audio.media.playing', true]],
    [AudioCommand.AudioMediaStop, ['audio.media.playing', false]],
    [AudioCommand.AudioPhonecallStart, ['phone.active', true]],
    [AudioCommand.AudioAttentionStart, ['phone.active', true]],
    [AudioCommand.AudioAttentionRinging, ['phone.active', true]],
    [AudioCommand.AudioPhonecallStop, ['phone.active', false]],
    [AudioCommand.AudioAttentionStop, ['phone.active', false]],
    [AudioCommand.AudioVoiceAssistantStart, ['voiceAssistant.active', true]],
    [AudioCommand.AudioVoiceAssistantStop, ['voiceAssistant.active', false]],
    [AudioCommand.AudioOutputStart, ['audio.system.playing', true]],
    [AudioCommand.AudioOutputStop, ['audio.system.playing', false]]
  ] as const)('AudioCommand %i → %s', (cmd, [path, expected]) => {
    const w = new StatusFileWriter(tmpFile(), { writeInitial: false })
    // Seed truthy/falsy opposite first so we observe the flip
    if (typeof expected === 'boolean' && expected === false) {
      // Start from `true` so the stop command must actually flip it
      if (path === 'audio.media.playing') w.setAudio('media', true)
      if (path === 'audio.system.playing') w.setAudio('system', true)
      if (path === 'phone.active') w.setPhoneCall(true)
      if (path === 'voiceAssistant.active') w.setVoiceAssistant(true)
    }
    w.applyAudioCommand(cmd)
    const state = w.getState() as unknown as Record<string, unknown>
    const parts = path.split('.')
    let v: unknown = state
    for (const p of parts) v = (v as Record<string, unknown>)[p]
    expect(v).toBe(expected)
  })

  test('NaviStart sets nav.announcing + audio.speech.playing simultaneously', () => {
    const w = new StatusFileWriter(tmpFile(), { writeInitial: false })
    w.applyAudioCommand(AudioCommand.AudioNaviStart)
    const s = w.getState()
    expect(s.nav.announcing).toBe(true)
    expect(s.audio.speech.playing).toBe(true)
  })

  test('TurnByTurnStart shares the nav/speech mapping with NaviStart', () => {
    const w = new StatusFileWriter(tmpFile(), { writeInitial: false })
    w.applyAudioCommand(AudioCommand.AudioTurnByTurnStart)
    expect(w.getState().nav.announcing).toBe(true)
    expect(w.getState().audio.speech.playing).toBe(true)
  })

  test('TurnByTurnStop clears both nav and speech', () => {
    const w = new StatusFileWriter(tmpFile(), { writeInitial: false })
    w.applyAudioCommand(AudioCommand.AudioNaviStart)
    w.applyAudioCommand(AudioCommand.AudioTurnByTurnStop)
    expect(w.getState().nav.announcing).toBe(false)
    expect(w.getState().audio.speech.playing).toBe(false)
  })

  test('unknown AudioCommand is silently ignored', () => {
    const w = new StatusFileWriter(tmpFile(), { writeInitial: false })
    const before = JSON.stringify(w.getState())
    w.applyAudioCommand(9999 as AudioCommand)
    expect(JSON.stringify(w.getState())).toBe(before)
  })
})

describe('StatusFileWriter — debounce + atomic write', () => {
  test('multiple setters within debounce window coalesce into a single write', () => {
    jest.useFakeTimers()
    const file = tmpFile()
    const w = new StatusFileWriter(file, { debounceMs: 50, writeInitial: false })
    const spy = jest.spyOn(fs, 'renameSync')
    w.setProjection('aa', 'AndroidAuto')
    w.setStreaming(true)
    w.setAudio('media', true)
    w.setPhoneCall(true)
    expect(spy).not.toHaveBeenCalled()
    jest.advanceTimersByTime(50)
    expect(spy).toHaveBeenCalledTimes(1)
    jest.useRealTimers()
    if (fs.existsSync(file)) fs.unlinkSync(file)
  })

  test('flush() forces an immediate write and clears any pending timer', () => {
    jest.useFakeTimers()
    const file = tmpFile()
    const w = new StatusFileWriter(file, { debounceMs: 50, writeInitial: false })
    w.setProjection('aa', 'AndroidAuto')
    w.flush()
    expect(fs.existsSync(file)).toBe(true)
    // Pending timer should have been cleared — advancing time must not cause another write
    const spy = jest.spyOn(fs, 'renameSync')
    jest.advanceTimersByTime(1000)
    expect(spy).not.toHaveBeenCalled()
    jest.useRealTimers()
    fs.unlinkSync(file)
  })

  test('write uses an atomic tmp + rename', () => {
    const file = tmpFile()
    const writeSpy = jest.spyOn(fs, 'writeFileSync')
    const renameSpy = jest.spyOn(fs, 'renameSync')
    new StatusFileWriter(file, { debounceMs: 0 })
    expect(writeSpy.mock.calls[0][0]).toBe(file + '.tmp')
    expect(renameSpy.mock.calls[0]).toEqual([file + '.tmp', file])
    if (fs.existsSync(file)) fs.unlinkSync(file)
  })

  test('write failure is swallowed and logged', () => {
    const file = tmpFile()
    jest.spyOn(fs, 'writeFileSync').mockImplementation(() => {
      throw new Error('ENOSPC')
    })
    const w = new StatusFileWriter(file, { debounceMs: 0, writeInitial: false })
    expect(() => w.flush()).not.toThrow()
  })
})

describe('StatusFileWriter — default file path', () => {
  test('uses app.getPath("userData") when no file is passed', () => {
    const w = new StatusFileWriter(undefined, { writeInitial: false })
    void w
    const expected = path.join(os.tmpdir(), 'statusData.json')
    w.setProjection('aa', 'AndroidAuto')
    w.flush()
    expect(fs.existsSync(expected)).toBe(true)
    fs.unlinkSync(expected)
  })
})
