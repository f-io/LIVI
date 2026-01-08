import { render, fireEvent, act } from '@testing-library/react'
import { Media } from '../Media'

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

describe('Media component', () => {
  beforeAll(() => {
    // — expand the global window
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    window.carplay = {
      ipc: { sendKeyCommand: jest.fn() },
      usb: {
        listenForEvents: jest.fn(),
        unlistenForEvents: jest.fn()
      }
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
      // @ts-ignore
    } as unknown as typeof window.carplay
  })

  beforeEach(() => {
    jest.useFakeTimers()
    // — expand the global window
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    window.carplay = {
      ipc: {
        sendKeyCommand: jest.fn()
      },
      usb: {
        listenForEvents: jest.fn(),
        unlistenForEvents: jest.fn()
      }
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
      // @ts-ignore
    } as unknown as typeof window.carplay
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
    // @ts-ignore
    expect(window.carplay.ipc.sendKeyCommand).toHaveBeenCalledWith('play')

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
    // @ts-ignore
    expect(window.carplay.ipc.sendKeyCommand).toHaveBeenCalledWith('pause')
  })

  it('sends next and prev commands', async () => {
    const { getByLabelText } = render(<Media />)

    await act(async () => {
      fireEvent.click(getByLabelText('Next'))
      fireEvent.click(getByLabelText('Previous'))
    })

    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    expect(window.carplay.ipc.sendKeyCommand).toHaveBeenCalledWith('next')
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    expect(window.carplay.ipc.sendKeyCommand).toHaveBeenCalledWith('prev')
  })

  it('cleans up USB listeners on unmount', () => {
    const { unmount } = render(<Media />)
    unmount()

    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    expect(window.carplay.usb.unlistenForEvents).toHaveBeenCalled()
  })
})
