import { act, render, screen, waitFor } from '@testing-library/react'
import { NavMini } from '../NavMini'

let onEventCb: ((event: unknown, ...args: unknown[]) => void) | undefined

const useLiviStoreMock = jest.fn()
const translateNavigationMock = jest.fn()
const useBlinkingTimeMock = jest.fn()

jest.mock('@store/store', () => ({
  useLiviStore: (selector: (state: { settings: { language: string } }) => unknown) =>
    useLiviStoreMock(selector)
}))

jest.mock('@shared/utils/translateNavigation', () => ({
  translateNavigation: (...args: unknown[]) => translateNavigationMock(...args)
}))

jest.mock('../../../../../hooks/useBlinkingTime', () => ({
  useBlinkingTime: () => useBlinkingTimeMock()
}))

describe('NavMini', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    onEventCb = undefined

    useLiviStoreMock.mockImplementation((selector: (state: any) => unknown) =>
      selector({
        settings: {
          language: 'de'
        }
      })
    )

    useBlinkingTimeMock.mockReturnValue('12:34')

    translateNavigationMock.mockImplementation((navi, locale) => ({
      RemainDistanceText: '200 m',
      TimeRemainingToDestinationText: '12 min',
      DistanceRemainingDisplayStringText: '5 km',
      CurrentRoadName: 'Main Street',
      codes: {
        ManeuverType: 2,
        TurnSide: 2
      },
      __navi: navi,
      __locale: locale
    }))
    ;(window as any).projection = {
      ipc: {
        readNavigation: jest.fn().mockResolvedValue({
          payload: {
            navi: {
              NaviStatus: 0
            }
          }
        }),
        onEvent: jest.fn((cb: (event: unknown, ...args: unknown[]) => void) => {
          onEventCb = cb
        }),
        offEvent: jest.fn()
      }
    }
  })

  test('shows blinking time when navigation is inactive', async () => {
    render(<NavMini />)

    await waitFor(() => {
      expect((window as any).projection.ipc.readNavigation).toHaveBeenCalled()
    })

    expect(
      screen.getAllByText((_, element) => element?.textContent === '12:34').length
    ).toBeGreaterThan(0)
    expect(screen.queryByText('200 m')).not.toBeInTheDocument()
  })

  test('renders active navigation content after hydrate', async () => {
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1,
          NaviManeuverType: 2,
          NaviTurnSide: 2
        }
      }
    })

    render(<NavMini iconSize={84} />)

    await waitFor(() => {
      expect(screen.getByText('200 m')).toBeInTheDocument()
    })

    expect(screen.getByText('12 min')).toBeInTheDocument()
    expect(screen.getByText('5 km')).toBeInTheDocument()
  })

  test('passes normalized locale from settings into translateNavigation', async () => {
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    await waitFor(() => {
      expect(translateNavigationMock).toHaveBeenCalled()
    })

    const lastCall = translateNavigationMock.mock.calls.at(-1)
    expect(lastCall?.[1]).toBe('de')
  })

  test('falls back to en locale for unsupported language', async () => {
    useLiviStoreMock.mockImplementation((selector: (state: any) => unknown) =>
      selector({
        settings: {
          language: 'fr'
        }
      })
    )
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    await waitFor(() => {
      expect(translateNavigationMock).toHaveBeenCalled()
    })

    const lastCall = translateNavigationMock.mock.calls.at(-1)
    expect(lastCall?.[1]).toBe('en')
  })

  test('subscribes to projection events and unsubscribes on unmount', () => {
    const { unmount } = render(<NavMini />)

    expect((window as any).projection.ipc.onEvent).toHaveBeenCalledTimes(1)
    expect(onEventCb).toBeDefined()

    const cb = onEventCb
    unmount()

    expect((window as any).projection.ipc.offEvent).toHaveBeenCalledWith(cb)
  })

  test('ignores non-navigation events', async () => {
    render(<NavMini />)

    await waitFor(() => {
      expect((window as any).projection.ipc.readNavigation).toHaveBeenCalled()
    })

    act(() => {
      onEventCb?.(null, { type: 'other', payload: { NaviStatus: 1 } })
    })

    expect(screen.queryByText('200 m')).not.toBeInTheDocument()
  })

  test('updates state from navigation event payload wrapper', async () => {
    render(<NavMini />)

    act(() => {
      onEventCb?.(null, {
        type: 'navigation',
        payload: {
          NaviStatus: 1,
          NaviManeuverType: 2,
          NaviTurnSide: 2
        }
      })
    })

    await waitFor(() => {
      expect(screen.getByText('200 m')).toBeInTheDocument()
    })
  })

  test('updates state from navigation event with nested navi payload', async () => {
    render(<NavMini />)

    act(() => {
      onEventCb?.(null, {
        type: 'navigation',
        payload: {
          navi: {
            NaviStatus: 1,
            NaviManeuverType: 2,
            NaviTurnSide: 2
          }
        }
      })
    })

    await waitFor(() => {
      expect(screen.getByText('200 m')).toBeInTheDocument()
    })
  })

  test('falls back to hydrate when navigation event patch cannot be unwrapped', async () => {
    ;(window as any).projection.ipc.readNavigation = jest
      .fn()
      .mockResolvedValueOnce({
        payload: {
          navi: {
            NaviStatus: 0
          }
        }
      })
      .mockResolvedValueOnce({
        payload: {
          navi: {
            NaviStatus: 1
          }
        }
      })

    render(<NavMini />)

    await waitFor(() => {
      expect((window as any).projection.ipc.readNavigation).toHaveBeenCalledTimes(1)
    })

    act(() => {
      onEventCb?.(null, {
        type: 'navigation',
        payload: {
          foo: 'bar'
        }
      })
    })

    await waitFor(() => {
      expect((window as any).projection.ipc.readNavigation).toHaveBeenCalledTimes(2)
    })

    await waitFor(() => {
      expect(screen.getByText('200 m')).toBeInTheDocument()
    })
  })

  test('renders maneuver image when NaviImageBase64 is present', async () => {
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1,
          NaviImageBase64: 'abc123'
        }
      }
    })

    render(<NavMini />)

    expect(await screen.findByAltText('Navigation maneuver')).toBeInTheDocument()
  })

  test('uses current road name when eta text is missing', async () => {
    translateNavigationMock.mockImplementation(() => ({
      RemainDistanceText: '200 m',
      TimeRemainingToDestinationText: '—',
      DistanceRemainingDisplayStringText: '5 km',
      CurrentRoadName: 'Main Street',
      codes: {
        ManeuverType: 2,
        TurnSide: 2
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    expect(await screen.findByText('Main Street')).toBeInTheDocument()
  })

  test('shows dash fallback when eta and road name are missing', async () => {
    translateNavigationMock.mockImplementation(() => ({
      RemainDistanceText: '200 m',
      TimeRemainingToDestinationText: '—',
      DistanceRemainingDisplayStringText: '5 km',
      CurrentRoadName: '',
      codes: {
        ManeuverType: 2,
        TurnSide: 2
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    expect(await screen.findAllByText('—')).not.toHaveLength(0)
  })

  test('hides destination distance in bottom row when maneuver image is present', async () => {
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1,
          NaviImageBase64: 'abc123'
        }
      }
    })

    render(<NavMini />)

    expect(await screen.findByAltText('Navigation maneuver')).toBeInTheDocument()
    expect(screen.queryByText('5 km')).not.toBeInTheDocument()
  })

  test('keeps previous navigation state when hydrate throws', async () => {
    ;(window as any).projection.ipc.readNavigation = jest
      .fn()
      .mockResolvedValueOnce({
        payload: {
          navi: {
            NaviStatus: 1
          }
        }
      })
      .mockRejectedValueOnce(new Error('read failed'))

    render(<NavMini />)

    await waitFor(() => {
      expect(screen.getByText('200 m')).toBeInTheDocument()
    })

    act(() => {
      onEventCb?.(null, {
        type: 'navigation',
        payload: {
          foo: 'bar'
        }
      })
    })

    await waitFor(() => {
      expect((window as any).projection.ipc.readNavigation).toHaveBeenCalledTimes(2)
    })

    expect(screen.getByText('200 m')).toBeInTheDocument()
  })

  test('renders straight icon when maneuver type is missing', async () => {
    translateNavigationMock.mockImplementation(() => ({
      RemainDistanceText: '200 m',
      TimeRemainingToDestinationText: '12 min',
      DistanceRemainingDisplayStringText: '5 km',
      CurrentRoadName: 'Main Street',
      codes: {
        ManeuverType: undefined,
        TurnSide: undefined
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    await waitFor(() => {
      expect(screen.getByTestId('StraightIcon')).toBeInTheDocument()
    })
  })

  test('renders roundabout icon for roundabout maneuver types', async () => {
    translateNavigationMock.mockImplementation(() => ({
      RemainDistanceText: '120 m',
      TimeRemainingToDestinationText: '9 min',
      DistanceRemainingDisplayStringText: '3 km',
      CurrentRoadName: 'Ring',
      codes: {
        ManeuverType: 28,
        TurnSide: 2
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    await waitFor(() => {
      expect(screen.getByTestId('RoundaboutRightIcon')).toBeInTheDocument()
    })
  })

  test('renders right u-turn icon when turn side is right', async () => {
    translateNavigationMock.mockImplementation(() => ({
      RemainDistanceText: '80 m',
      TimeRemainingToDestinationText: '4 min',
      DistanceRemainingDisplayStringText: '1 km',
      CurrentRoadName: 'Main Street',
      codes: {
        ManeuverType: 4,
        TurnSide: 2
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    await waitFor(() => {
      expect(screen.getByTestId('UTurnRightIcon')).toBeInTheDocument()
    })
  })

  test('renders fallback help icon for unknown maneuver type', async () => {
    translateNavigationMock.mockImplementation(() => ({
      RemainDistanceText: '300 m',
      TimeRemainingToDestinationText: '6 min',
      DistanceRemainingDisplayStringText: '3 km',
      CurrentRoadName: 'Mystery Road',
      codes: {
        ManeuverType: 999,
        TurnSide: 1
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    await waitFor(() => {
      expect(screen.getByTestId('HelpOutlinedIcon')).toBeInTheDocument()
    })
  })

  test('renders exit icon for exit maneuver type', async () => {
    translateNavigationMock.mockImplementation(() => ({
      RemainDistanceText: '500 m',
      TimeRemainingToDestinationText: '15 min',
      DistanceRemainingDisplayStringText: '8 km',
      CurrentRoadName: 'Highway',
      codes: {
        ManeuverType: 8,
        TurnSide: 1
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    await waitFor(() => {
      expect(screen.getByTestId('ExitToAppIcon')).toBeInTheDocument()
    })
  })

  test('renders sharp right icon for maneuver type 48', async () => {
    translateNavigationMock.mockImplementation(() => ({
      RemainDistanceText: '60 m',
      TimeRemainingToDestinationText: '2 min',
      DistanceRemainingDisplayStringText: '600 m',
      CurrentRoadName: 'Sharp Turn Rd',
      codes: {
        ManeuverType: 48,
        TurnSide: 2
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavMini />)

    await waitFor(() => {
      expect(screen.getByTestId('TurnSharpRightIcon')).toBeInTheDocument()
    })
  })
})
