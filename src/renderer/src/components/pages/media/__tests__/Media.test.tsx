import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Media } from '../Media'

jest.mock('../components/createFFTSpectrum', () => ({
  FFTSpectrum: () => null
}))

jest.mock('./../hooks/useBelowNavTop', () => ({
  useBelowNavTop: () => 0
}))

jest.mock('./../hooks/useElementSize', () => ({
  useElementSize: () => [{ current: null }, { w: 600, h: 400 }]
}))

jest.mock('./../hooks/useMediaState', () => ({
  useMediaState: () => ({
    snap: {
      payload: {
        media: {
          MediaSongName: 'Track',
          MediaArtistName: 'Artist',
          MediaAlbumName: 'Album',
          MediaAPPName: 'CarPlay',
          MediaSongDuration: 1000,
          MediaPlayStatus: 0
        }
      }
    },
    livePlayMs: 100
  })
}))

let usbEventCb: ((_: unknown, ...args: unknown[]) => void) | undefined

describe('Media component', () => {
  beforeAll(() => {
    // — expand the global window
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    window.projection = {
      ipc: { sendCommand: jest.fn(), setVisualizerEnabled: jest.fn() },
      usb: {
        listenForEvents: jest.fn(),
        unlistenForEvents: jest.fn()
      }
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    } as unknown as typeof window.projection
  })

  beforeEach(() => {
    usbEventCb = undefined
    jest.useFakeTimers()
    // — expand the global window
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    window.projection = {
      ipc: {
        sendCommand: jest.fn(),
        setVisualizerEnabled: jest.fn()
      },
      usb: {
        listenForEvents: jest.fn((cb: any) => {
          usbEventCb = cb
        }),
        unlistenForEvents: jest.fn()
      }
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    } as unknown as typeof window.projection
  })

  afterEach(() => {
    jest.clearAllMocks()
    jest.useRealTimers()
  })

  it('sends play/pause command and resets press feedback', async () => {
    const { getByLabelText } = render(<Media />)
    const playButton = getByLabelText('Play/Pause')

    // simulate play click
    await act(async () => {
      fireEvent.click(playButton)
    })

    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    expect(window.projection.ipc.sendCommand).toHaveBeenCalledWith('play')

    // advance timers for reset
    await act(async () => {
      jest.advanceTimersByTime(150)
    })

    // simulate second click (pause)
    await act(async () => {
      fireEvent.click(playButton)
      jest.advanceTimersByTime(150)
    })

    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    expect(window.projection.ipc.sendCommand).toHaveBeenCalledWith('pause')
  })

  it('sends next and prev commands', async () => {
    const { getByLabelText } = render(<Media />)

    await act(async () => {
      fireEvent.click(getByLabelText('Next'))
      fireEvent.click(getByLabelText('Previous'))
    })

    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    expect(window.projection.ipc.sendCommand).toHaveBeenCalledWith('next')
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    expect(window.projection.ipc.sendCommand).toHaveBeenCalledWith('prev')
  })

  it('cleans up USB listeners on unmount', () => {
    const { unmount } = render(<Media />)
    unmount()

    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    expect(window.projection.usb.unlistenForEvents).toHaveBeenCalled()
  })

  it('artwork button toggles FFT spectrum on click', async () => {
    render(<Media />)
    // showFft=false initially
    expect(screen.getByRole('button', { name: /Show spectrum/i })).toBeInTheDocument()

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Show spectrum/i }))
    })
    // showFft=true → label flips
    expect(screen.getByRole('button', { name: /Show artwork/i })).toBeInTheDocument()

    // Toggle back
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Show artwork/i }))
    })
    expect(screen.getByRole('button', { name: /Show spectrum/i })).toBeInTheDocument()
  })

  it('keyboard Enter on artwork button toggles FFT', async () => {
    render(<Media />)
    await act(async () => {
      fireEvent.keyDown(screen.getByRole('button', { name: /Show spectrum/i }), { key: 'Enter' })
    })
    expect(screen.getByRole('button', { name: /Show artwork/i })).toBeInTheDocument()
  })

  it('keyboard Space on artwork button toggles FFT', async () => {
    render(<Media />)
    await act(async () => {
      fireEvent.keyDown(screen.getByRole('button', { name: /Show spectrum/i }), { key: ' ' })
    })
    expect(screen.getByRole('button', { name: /Show artwork/i })).toBeInTheDocument()
  })

  it('car-media-key PLAY event bumps play feedback', async () => {
    render(<Media />)

    await act(async () => {
      window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command: 'play' } }))
    })
    // Component handled the event without error
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    expect(window.projection.ipc.sendCommand).not.toHaveBeenCalled() // car-media-key doesn't call sendCommand
  })

  it('car-media-key NEXT event flashes next button', async () => {
    render(<Media />)

    await act(async () => {
      window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command: 'next' } }))
    })
    expect(screen.getByLabelText('Next')).toBeInTheDocument()
  })

  it('car-media-key PREV event flashes prev button', async () => {
    render(<Media />)

    await act(async () => {
      window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command: 'prev' } }))
    })
    expect(screen.getByLabelText('Previous')).toBeInTheDocument()
  })

  it('car-media-key with no command does nothing', async () => {
    render(<Media />)

    await act(async () => {
      window.dispatchEvent(new CustomEvent('car-media-key', { detail: {} }))
    })
    expect(screen.getByLabelText('Play/Pause')).toBeInTheDocument()
  })

  it('USB unplugged event resets showFft to false', async () => {
    render(<Media />)

    // First enable FFT
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Show spectrum/i }))
    })
    expect(screen.getByRole('button', { name: /Show artwork/i })).toBeInTheDocument()

    // USB unplug resets showFft to false
    await act(async () => {
      usbEventCb?.(null, { type: 'unplugged' })
    })
    expect(screen.getByRole('button', { name: /Show spectrum/i })).toBeInTheDocument()
  })
})
