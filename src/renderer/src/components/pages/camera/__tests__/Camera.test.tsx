import { act, render, screen, waitFor } from '@testing-library/react'
import { Camera } from '../Camera'

let mockSettings: any = null

jest.mock('@store/store', () => ({
  useLiviStore: (selector: (s: { settings: unknown }) => unknown) =>
    selector({ settings: mockSettings })
}))

describe('pages/camera Camera', () => {
  const addEventListener = jest.fn()
  const removeEventListener = jest.fn()
  const enumerateDevices = jest.fn()
  const getUserMedia = jest.fn()

  const createStream = () => {
    const track = { stop: jest.fn(), getSettings: jest.fn(() => ({ width: 1280, height: 720 })) }
    return {
      getTracks: () => [track],
      getVideoTracks: () => [track]
    } as unknown as MediaStream
  }

  beforeEach(() => {
    jest.clearAllMocks()
    mockSettings = { camera: 'cam-1', cameraMirror: false }

    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: {
        addEventListener,
        removeEventListener,
        enumerateDevices,
        getUserMedia
      }
    })

    Object.defineProperty(HTMLMediaElement.prototype, 'play', {
      configurable: true,
      value: jest.fn(() => Promise.resolve())
    })

    Object.defineProperty(HTMLMediaElement.prototype, 'pause', {
      configurable: true,
      value: jest.fn()
    })
  })

  test('opens exact configured camera', async () => {
    enumerateDevices.mockResolvedValue([{ kind: 'videoinput', deviceId: 'cam-1' }])
    getUserMedia.mockResolvedValue(createStream())

    render(<Camera />)

    expect(screen.getByText('Opening camera…')).toBeInTheDocument()

    await waitFor(() => {
      expect(getUserMedia).toHaveBeenCalled()
    })

    expect(screen.queryByText('Using fallback camera')).not.toBeInTheDocument()
    expect(addEventListener).toHaveBeenCalledWith('devicechange', expect.any(Function))
  })

  test('falls back when exact camera cannot be opened', async () => {
    enumerateDevices.mockResolvedValue([{ kind: 'videoinput', deviceId: 'cam-1' }])
    getUserMedia.mockImplementation(async (constraints: MediaStreamConstraints) => {
      const video = constraints.video
      if (video && typeof video === 'object' && 'deviceId' in video) {
        const did = (video as MediaTrackConstraints).deviceId
        if (did && typeof did === 'object' && 'exact' in did) {
          throw new Error('exact failed')
        }
      }
      return createStream()
    })

    render(<Camera showFallbackNotice />)

    await waitFor(() => {
      expect(screen.getByText('Using fallback camera')).toBeInTheDocument()
    })

    expect(getUserMedia.mock.calls.length).toBeGreaterThan(1)
  })

  test('shows error when camera not configured and fallback disabled', async () => {
    mockSettings = { camera: '' }

    render(<Camera allowFallback={false} />)

    expect(await screen.findByText('No camera configured.')).toBeInTheDocument()
    expect(getUserMedia).not.toHaveBeenCalled()
  })

  test('shows not found error when saved camera is missing and fallback disabled', async () => {
    enumerateDevices.mockResolvedValue([{ kind: 'videoinput', deviceId: 'another' }])
    getUserMedia.mockRejectedValue(new Error('should not be used'))

    render(<Camera allowFallback={false} />)

    expect(await screen.findByText('Saved camera not found.')).toBeInTheDocument()
  })

  test('handles devicechange and cleans up stream on unmount', async () => {
    const stream = createStream()
    enumerateDevices.mockResolvedValue([{ kind: 'videoinput', deviceId: 'cam-1' }])
    getUserMedia.mockResolvedValue(stream)

    const { unmount } = render(<Camera />)

    await waitFor(() => {
      expect(getUserMedia).toHaveBeenCalled()
    })

    const deviceChangeHandler = addEventListener.mock.calls.find(
      (c) => c[0] === 'devicechange'
    )?.[1]
    expect(deviceChangeHandler).toBeTruthy()

    await act(async () => {
      await deviceChangeHandler()
    })
    expect(getUserMedia).toHaveBeenCalledTimes(2)

    unmount()

    expect(removeEventListener).toHaveBeenCalledWith('devicechange', deviceChangeHandler)
    expect(HTMLMediaElement.prototype.pause as jest.Mock).toHaveBeenCalled()
  })
})
