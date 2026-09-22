import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

const { addon, addonRef } = vi.hoisted(() => {
  const addon = {
    openAudio: vi.fn((): unknown => ({ id: 'stream' })),
    audioStreamId: vi.fn(() => 9),
    setAudioActive: vi.fn(),
    pushAudio: vi.fn(() => true),
    setAudioVolume: vi.fn(),
    closeAudio: vi.fn()
  }
  return { addon, addonRef: { current: addon as unknown } }
})

vi.mock('@main/services/video/GstVideo', () => ({
  useHostProcess: false,
  gstAddon: () => addonRef.current
}))
vi.mock('@main/services/video/gstHost', () => ({ gstHost: {} }))

import { HostAudioOutput } from '../HostAudioOutput'

beforeEach(() => {
  addonRef.current = addon
  addon.openAudio.mockClear().mockReturnValue({ id: 'stream' })
  addon.audioStreamId.mockClear().mockReturnValue(9)
  addon.setAudioActive.mockClear()
  addon.pushAudio.mockClear().mockReturnValue(true)
  addon.setAudioVolume.mockClear()
  addon.closeAudio.mockClear()
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('HostAudioOutput in the in-process addon', () => {
  test('opens through the addon, streams, sets the volume and closes', () => {
    const onOpened = vi.fn()
    const out = new HostAudioOutput({ sampleRate: 24000, channels: 2, device: 'src0', onOpened })
    out.start()
    expect(addon.openAudio).toHaveBeenCalledWith(24000, 2, 'src0', false)
    expect(addon.setAudioActive).toHaveBeenCalledWith({ id: 'stream' }, true)
    expect(onOpened).toHaveBeenCalledWith(9)
    expect(out.hostStreamId).toBe(9)

    out.write(new Int16Array([1, 2, 3]))
    expect(addon.pushAudio).toHaveBeenCalledWith({ id: 'stream' }, expect.any(Buffer))
    out.setVolume(0.5, 40)
    expect(addon.setAudioVolume).toHaveBeenCalledWith({ id: 'stream' }, 0.5, 40)
    out.stop()
    expect(addon.closeAudio).toHaveBeenCalledWith({ id: 'stream' })
    // A write after stop is dropped.
    out.write(new Int16Array([9]))
    expect(addon.pushAudio).toHaveBeenCalledTimes(1)
  })

  test('buffers writes made before the stream opens and flushes them on start', () => {
    const out = new HostAudioOutput({ sampleRate: 16000, channels: 1 })
    out.write(Buffer.from([1, 2]))
    expect(addon.pushAudio).not.toHaveBeenCalled()
    out.start()
    expect(addon.pushAudio).toHaveBeenCalledTimes(1)
  })

  test('does nothing when the addon is absent', () => {
    addonRef.current = null
    const out = new HostAudioOutput({ sampleRate: 16000, channels: 1 })
    out.start()
    expect(out.hostStreamId).toBeNull()
    out.setVolume(0.5)
    out.stop()
  })

  test('warns and stays closed when the addon cannot open the stream', () => {
    addon.openAudio.mockReturnValueOnce(null)
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const out = new HostAudioOutput({ sampleRate: 16000, channels: 1 })
    out.start()
    expect(out.hostStreamId).toBeNull()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })
})
