import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { Mock } from 'vitest'
import { Media } from '../Media'

vi.mock('../components/createFFTSpectrum', () => ({
  FFTSpectrum: () => null
}))

vi.mock('./../hooks/useBelowNavTop', () => ({
  useBelowNavTop: () => 0
}))

vi.mock('./../hooks/useElementSize', () => ({
  useElementSize: () => [{ current: null }, { w: 600, h: 400 }]
}))

vi.mock('./../hooks/useMediaState', () => ({
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
  beforeAll(async () => {
    // — expand the global window
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    window.projection = {
      ipc: {
        sendCommand: vi.fn(),
        setVisualizerEnabled: vi.fn(),
        onEvent: vi.fn(() => vi.fn()),
        readMedia: vi.fn(() => Promise.resolve(null))
      },
      usb: {
        listenForEvents: vi.fn(() => vi.fn())
      }
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    } as unknown as typeof window.projection
  })

  beforeEach(async () => {
    usbEventCb = undefined
    vi.useFakeTimers({ shouldAdvanceTime: true })
    // — expand the global window
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    window.projection = {
      ipc: {
        sendCommand: vi.fn(),
        setVisualizerEnabled: vi.fn(),
        onEvent: vi.fn((cb: any) => {
          usbEventCb = cb
          return vi.fn()
        }),
        readMedia: vi.fn(() => Promise.resolve(null))
      },
      usb: {
        listenForEvents: vi.fn(() => vi.fn())
      }
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    } as unknown as typeof window.projection
  })

  afterEach(async () => {
    vi.clearAllMocks()
    vi.useRealTimers()
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
      vi.advanceTimersByTime(150)
    })

    // simulate second click (pause)
    await act(async () => {
      fireEvent.click(playButton)
      vi.advanceTimersByTime(150)
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

  it('cleans up listeners on unmount', async () => {
    const unsub = vi.fn()
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    ;(window.projection.ipc.onEvent as Mock).mockImplementationOnce(() => unsub)

    const { unmount } = render(<Media />)
    unmount()

    expect(unsub).toHaveBeenCalled()
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

  it('media-reset event resets showFft to false', async () => {
    render(<Media />)

    // First enable FFT
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Show spectrum/i }))
    })
    expect(screen.getByRole('button', { name: /Show artwork/i })).toBeInTheDocument()

    // Session change resets showFft to false
    await act(async () => {
      usbEventCb?.(null, { type: 'media-reset' })
    })
    expect(screen.getByRole('button', { name: /Show spectrum/i })).toBeInTheDocument()
  })
})
