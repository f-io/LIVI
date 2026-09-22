import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

const {
  openMicTapHost,
  closeMicTapHost,
  addonHandle,
  openMicTapAddon,
  closeMicTapAddon,
  gstAddonMock,
  hostState
} = vi.hoisted(() => {
  const addonHandle = { id: 'tap' }
  return {
    addonHandle,
    openMicTapHost: vi.fn(() => 7),
    closeMicTapHost: vi.fn(),
    openMicTapAddon: vi.fn(() => addonHandle as unknown),
    closeMicTapAddon: vi.fn(),
    gstAddonMock: vi.fn(),
    hostState: { useHostProcess: true }
  }
})

vi.mock('@main/services/video/GstVideo', () => ({
  get useHostProcess() {
    return hostState.useHostProcess
  },
  gstAddon: () => gstAddonMock()
}))
vi.mock('@main/services/video/gstHost', () => ({
  gstHost: { openMicTap: openMicTapHost, closeMicTap: closeMicTapHost }
}))

import { MicTap } from '../micTap'

beforeEach(() => {
  openMicTapHost.mockClear().mockReturnValue(7)
  closeMicTapHost.mockClear()
  openMicTapAddon.mockClear().mockReturnValue(addonHandle)
  closeMicTapAddon.mockClear()
  gstAddonMock
    .mockClear()
    .mockReturnValue({ openMicTap: openMicTapAddon, closeMicTap: closeMicTapAddon })
})

afterEach(() => {
  hostState.useHostProcess = true
})

describe('MicTap on the host process', () => {
  test('opens the tap in the host and closes it by its id', () => {
    hostState.useHostProcess = true
    const tap = MicTap.open('/tmp/mic.sock', { sampleRate: 16000, channels: 1, device: 'src0' })
    expect(openMicTapHost).toHaveBeenCalledWith('/tmp/mic.sock', {
      sampleRate: 16000,
      channels: 1,
      device: 'src0'
    })
    tap?.close()
    expect(closeMicTapHost).toHaveBeenCalledWith(7)
    tap?.close()
    expect(closeMicTapHost).toHaveBeenCalledTimes(2)
  })
})

describe('MicTap in the in-process addon', () => {
  test('opens the tap through the addon and closes the handle once', () => {
    hostState.useHostProcess = false
    const tap = MicTap.open('/tmp/mic.sock', { sampleRate: 24000, channels: 2, device: 'src1' })
    expect(openMicTapAddon).toHaveBeenCalledWith('/tmp/mic.sock', 24000, 2, 'src1')
    tap?.close()
    expect(closeMicTapAddon).toHaveBeenCalledWith(addonHandle)
    tap?.close()
    expect(closeMicTapAddon).toHaveBeenCalledTimes(1)
  })

  test('returns null when the addon is absent', () => {
    hostState.useHostProcess = false
    gstAddonMock.mockReturnValueOnce(null as never)
    expect(MicTap.open('/tmp/mic.sock', { sampleRate: 16000, channels: 1 })).toBeNull()
  })

  test('returns null when the addon cannot open the tap', () => {
    hostState.useHostProcess = false
    openMicTapAddon.mockReturnValueOnce(null as never)
    expect(MicTap.open('/tmp/mic.sock', { sampleRate: 16000, channels: 1 })).toBeNull()
  })
})
