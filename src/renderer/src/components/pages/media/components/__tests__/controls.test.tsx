import React from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { Controls } from '../controls'

const circleBtnStyleMock = jest.fn((size: number, state: unknown) => ({
  width: size,
  height: size,
  border: 'none',
  ...(state as object)
}))

jest.mock('../../styles', () => ({
  circleBtnStyle: (...args: Parameters<typeof circleBtnStyleMock>) => circleBtnStyleMock(...args)
}))

jest.mock('@mui/material/styles', () => ({
  useTheme: () => ({
    palette: {
      primary: {
        main: '#00aaff'
      }
    }
  })
}))

jest.mock('@mui/icons-material/PlayArrow', () => ({
  __esModule: true,
  default: ({ sx }: { sx?: Record<string, unknown> }) => (
    <span data-testid="play-icon" data-sx={JSON.stringify(sx ?? {})}>
      PlayIcon
    </span>
  )
}))

jest.mock('@mui/icons-material/Pause', () => ({
  __esModule: true,
  default: ({ sx }: { sx?: Record<string, unknown> }) => (
    <span data-testid="pause-icon" data-sx={JSON.stringify(sx ?? {})}>
      PauseIcon
    </span>
  )
}))

jest.mock('@mui/icons-material/SkipNext', () => ({
  __esModule: true,
  default: ({ sx }: { sx?: Record<string, unknown> }) => (
    <span data-testid="next-icon" data-sx={JSON.stringify(sx ?? {})}>
      NextIcon
    </span>
  )
}))

jest.mock('@mui/icons-material/SkipPrevious', () => ({
  __esModule: true,
  default: ({ sx }: { sx?: Record<string, unknown> }) => (
    <span data-testid="prev-icon" data-sx={JSON.stringify(sx ?? {})}>
      PrevIcon
    </span>
  )
}))

describe('Controls', () => {
  const onPrev = jest.fn()
  const onPlayPause = jest.fn()
  const onNext = jest.fn()
  const onSetFocus = jest.fn()

  const prevBtnRef = React.createRef<HTMLButtonElement>()
  const playBtnRef = React.createRef<HTMLButtonElement>()
  const nextBtnRef = React.createRef<HTMLButtonElement>()

  const renderControls = (overrides?: Partial<React.ComponentProps<typeof Controls>>) =>
    render(
      <Controls
        ctrlGap={12}
        ctrlSize={40}
        prevBtnRef={prevBtnRef}
        playBtnRef={playBtnRef}
        nextBtnRef={nextBtnRef}
        onSetFocus={onSetFocus}
        onPrev={onPrev}
        onPlayPause={onPlayPause}
        onNext={onNext}
        uiPlaying={false}
        press={{ play: false, next: false, prev: false }}
        focus={{ play: false, next: false, prev: false }}
        iconPx={18}
        iconMainPx={24}
        {...overrides}
      />
    )

  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('renders previous, play/pause and next buttons', () => {
    renderControls()

    expect(screen.getByLabelText('Previous')).toBeInTheDocument()
    expect(screen.getByLabelText('Play/Pause')).toBeInTheDocument()
    expect(screen.getByLabelText('Next')).toBeInTheDocument()
    expect(screen.getByTestId('play-icon')).toBeInTheDocument()
  })

  test('renders pause icon and pressed state when uiPlaying is true', () => {
    renderControls({ uiPlaying: true })

    const playPauseButton = screen.getByLabelText('Play/Pause')

    expect(screen.getByTestId('pause-icon')).toBeInTheDocument()
    expect(screen.queryByTestId('play-icon')).not.toBeInTheDocument()
    expect(playPauseButton).toHaveAttribute('title', 'Pause')
    expect(playPauseButton).toHaveAttribute('aria-pressed', 'true')
  })

  test('calls control handlers on click', () => {
    renderControls()

    fireEvent.click(screen.getByLabelText('Previous'))
    fireEvent.click(screen.getByLabelText('Play/Pause'))
    fireEvent.click(screen.getByLabelText('Next'))

    expect(onPrev).toHaveBeenCalledTimes(1)
    expect(onPlayPause).toHaveBeenCalledTimes(1)
    expect(onNext).toHaveBeenCalledTimes(1)
  })

  test('calls onSetFocus updater on focus and blur for each button', () => {
    renderControls()

    const prevButton = screen.getByLabelText('Previous')
    const playButton = screen.getByLabelText('Play/Pause')
    const nextButton = screen.getByLabelText('Next')

    fireEvent.focus(prevButton)
    fireEvent.blur(prevButton)
    fireEvent.focus(playButton)
    fireEvent.blur(playButton)
    fireEvent.focus(nextButton)
    fireEvent.blur(nextButton)

    expect(onSetFocus).toHaveBeenCalledTimes(6)

    const prevFocusOn = onSetFocus.mock.calls[0][0]({ play: false, next: false, prev: false })
    const prevFocusOff = onSetFocus.mock.calls[1][0]({ play: false, next: false, prev: true })
    const playFocusOn = onSetFocus.mock.calls[2][0]({ play: false, next: false, prev: false })
    const playFocusOff = onSetFocus.mock.calls[3][0]({ play: true, next: false, prev: false })
    const nextFocusOn = onSetFocus.mock.calls[4][0]({ play: false, next: false, prev: false })
    const nextFocusOff = onSetFocus.mock.calls[5][0]({ play: false, next: true, prev: false })

    expect(prevFocusOn).toEqual({ play: false, next: false, prev: true })
    expect(prevFocusOff).toEqual({ play: false, next: false, prev: false })
    expect(playFocusOn).toEqual({ play: true, next: false, prev: false })
    expect(playFocusOff).toEqual({ play: false, next: false, prev: false })
    expect(nextFocusOn).toEqual({ play: false, next: true, prev: false })
    expect(nextFocusOff).toEqual({ play: false, next: false, prev: false })
  })

  test('blurs button on mouse up', () => {
    renderControls()

    const playButton = screen.getByLabelText('Play/Pause')
    const blurSpy = jest.spyOn(playButton, 'blur')

    fireEvent.mouseUp(playButton)

    expect(blurSpy).toHaveBeenCalledTimes(1)
  })

  test('updates hover state through circleBtnStyle calls', () => {
    renderControls()

    const nextButton = screen.getByLabelText('Next')

    const beforeHoverCalls = circleBtnStyleMock.mock.calls.length
    fireEvent.mouseEnter(nextButton)
    fireEvent.mouseLeave(nextButton)

    const afterCalls = circleBtnStyleMock.mock.calls.slice(beforeHoverCalls)

    expect(afterCalls.some(([, state]) => (state as { hovered?: boolean }).hovered === true)).toBe(
      true
    )
    expect(afterCalls.some(([, state]) => (state as { hovered?: boolean }).hovered === false)).toBe(
      true
    )
  })

  test('passes ring color, sizes, press and focus state to circleBtnStyle', () => {
    renderControls({
      uiPlaying: true,
      press: { prev: true, play: true, next: false },
      focus: { prev: false, play: true, next: true }
    })

    expect(circleBtnStyleMock).toHaveBeenCalledWith(
      40,
      expect.objectContaining({
        pressed: true,
        focused: false,
        hovered: false,
        ringColor: '#00aaff'
      })
    )

    expect(circleBtnStyleMock).toHaveBeenCalledWith(
      44,
      expect.objectContaining({
        pressed: true,
        focused: true,
        hovered: false,
        ringColor: '#00aaff'
      })
    )

    expect(circleBtnStyleMock).toHaveBeenCalledWith(
      40,
      expect.objectContaining({
        pressed: false,
        focused: true,
        hovered: false,
        ringColor: '#00aaff'
      })
    )
  })

  test('assigns refs to rendered buttons', () => {
    renderControls()

    expect(prevBtnRef.current).toBe(screen.getByLabelText('Previous'))
    expect(playBtnRef.current).toBe(screen.getByLabelText('Play/Pause'))
    expect(nextBtnRef.current).toBe(screen.getByLabelText('Next'))
  })
})
