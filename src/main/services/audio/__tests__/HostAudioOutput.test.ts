import { gstHost } from '@main/services/video/gstHost'
import { HostAudioOutput } from '../HostAudioOutput'

vi.mock('@main/services/video/gstHost', () => ({
  gstHost: {
    openAudio: vi.fn(),
    setAudioActive: vi.fn(),
    pushAudio: vi.fn(),
    setAudioVolume: vi.fn(),
    closeAudio: vi.fn()
  }
}))

// The host path, whatever platform runs the tests.
vi.mock('@main/services/video/GstVideo', () => ({
  useHostProcess: true,
  gstAddon: () => null
}))

const mocked = vi.mocked(gstHost)

function opened(streamId = 5): void {
  mocked.openAudio.mockResolvedValue({ streamId, dataPort: 0, controlPort: 0 })
}

describe('HostAudioOutput', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    opened()
  })

  test('opens a fed stream in the host and switches it on', async () => {
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2, device: 'sink0' })

    out.start()
    await vi.waitFor(() => expect(mocked.setAudioActive).toHaveBeenCalledWith(5, true))

    expect(mocked.openAudio.mock.calls[0][1]).toMatchObject({
      codec: 'pcm-le',
      clockRate: 48000,
      channels: 2,
      fed: true,
      realtime: false,
      device: 'sink0'
    })
  })

  test('a second start opens nothing more', async () => {
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })

    out.start()
    out.start()
    await vi.waitFor(() => expect(mocked.setAudioActive).toHaveBeenCalled())
    out.start()

    expect(mocked.openAudio).toHaveBeenCalledTimes(1)
  })

  test('the host stream volume is set on the host', async () => {
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()
    await vi.waitFor(() => expect(out.hostStreamId).toBe(5))
    out.setVolume(0.4, 60)
    expect(mocked.setAudioVolume).toHaveBeenCalledWith(5, 0.4, 60)
  })

  test('samples written after the stream opens go straight to the host', async () => {
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()
    await vi.waitFor(() => expect(out.hostStreamId).toBe(5))
    mocked.pushAudio.mockClear()
    out.write(new Int16Array([1, 2, 3]))
    expect(mocked.pushAudio).toHaveBeenCalledWith(5, expect.any(Buffer))
  })

  test('samples written before the stream is open follow once it is', async () => {
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()

    out.write(Buffer.from([1, 2]))
    expect(mocked.pushAudio).not.toHaveBeenCalled()

    await vi.waitFor(() => expect(mocked.pushAudio).toHaveBeenCalledWith(5, Buffer.from([1, 2])))

    out.write(Buffer.from([3]))
    expect(mocked.pushAudio).toHaveBeenLastCalledWith(5, Buffer.from([3]))
  })

  test('an Int16Array is written as its bytes', async () => {
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()
    await vi.waitFor(() => expect(mocked.setAudioActive).toHaveBeenCalled())

    out.write(Int16Array.from([1, -1]))

    expect(mocked.pushAudio).toHaveBeenCalledWith(5, Buffer.from([1, 0, 255, 255]))
  })

  test('nothing is kept beyond the pending bound', async () => {
    mocked.openAudio.mockReturnValue(new Promise(() => {}))
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()

    for (let i = 0; i < 70; i += 1) out.write(Buffer.from([i]))

    expect(mocked.pushAudio).not.toHaveBeenCalled()
  })

  test('an empty write is ignored', () => {
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })

    out.write(null)
    out.write(undefined)

    expect(mocked.pushAudio).not.toHaveBeenCalled()
  })

  test('stopping closes the stream and refuses further samples', async () => {
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()
    await vi.waitFor(() => expect(mocked.setAudioActive).toHaveBeenCalled())

    out.stop()
    out.write(Buffer.from([1]))

    expect(mocked.closeAudio).toHaveBeenCalledWith(5)
    expect(mocked.pushAudio).not.toHaveBeenCalled()
  })

  test('a stream that opens after the stop is closed at once', async () => {
    let settle: (v: { streamId: number; dataPort: number; controlPort: number }) => void = () => {}
    mocked.openAudio.mockReturnValue(new Promise((r) => (settle = r)))
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })
    out.start()

    out.stop()
    settle({ streamId: 9, dataPort: 0, controlPort: 0 })
    await vi.waitFor(() => expect(mocked.closeAudio).toHaveBeenCalledWith(9))

    expect(mocked.setAudioActive).not.toHaveBeenCalled()
  })

  test('a failed open leaves the stream closed', async () => {
    mocked.openAudio.mockRejectedValue(new Error('no host'))
    const out = new HostAudioOutput({ sampleRate: 48000, channels: 2 })

    out.start()
    await vi.waitFor(() => expect(mocked.openAudio).toHaveBeenCalled())
    out.write(Buffer.from([1]))

    expect(mocked.pushAudio).not.toHaveBeenCalled()
  })
})
