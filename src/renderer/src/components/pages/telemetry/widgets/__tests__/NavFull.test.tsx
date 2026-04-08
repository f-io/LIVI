import { act, render, screen, waitFor } from '@testing-library/react'
import { NavFull } from '../NavFull'

let onEventCb: ((event: unknown, ...args: unknown[]) => void) | undefined

const useLiviStoreMock = jest.fn()
const translateNavigationMock = jest.fn()

jest.mock('@store/store', () => ({
  useLiviStore: (selector: (state: { settings: { language: string } }) => unknown) =>
    useLiviStoreMock(selector)
}))

jest.mock('@shared/utils/translateNavigation', () => ({
  translateNavigation: (...args: unknown[]) => translateNavigationMock(...args)
}))

describe('NavFull', () => {
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

    translateNavigationMock.mockImplementation((navi, locale) => ({
      ManeuverTypeText: 'Turn right',
      RemainDistanceText: '200 m',
      CurrentRoadName: 'Main Street',
      TimeRemainingToDestinationText: '12 min',
      DistanceRemainingDisplayStringText: '5 km',
      DestinationName: 'Berlin',
      SourceName: 'Maps',
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

  test('shows inactive navigation icon when navigation is not active', async () => {
    render(<NavFull />)

    await waitFor(() => {
      expect((window as any).projection.ipc.readNavigation).toHaveBeenCalled()
    })

    expect(screen.queryByText('Turn right')).not.toBeInTheDocument()
  })

  test('hydrates navigation snapshot and renders active navigation content', async () => {
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1,
          NaviManeuverType: 2,
          NaviTurnSide: 2
        }
      }
    })

    render(<NavFull />)

    await waitFor(() => {
      expect(screen.getByText('Turn right')).toBeInTheDocument()
    })

    expect(screen.getByText('200 m')).toBeInTheDocument()
    expect(screen.getByText('Main Street')).toBeInTheDocument()
    expect(screen.getByText('12 min')).toBeInTheDocument()
    expect(screen.getByText('5 km')).toBeInTheDocument()
    expect(screen.getByText('Berlin')).toBeInTheDocument()
    expect(screen.getByText('Maps')).toBeInTheDocument()
  })

  test('passes normalized locale from settings into translateNavigation', async () => {
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1
        }
      }
    })

    render(<NavFull />)

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

    render(<NavFull />)

    await waitFor(() => {
      expect(translateNavigationMock).toHaveBeenCalled()
    })

    const lastCall = translateNavigationMock.mock.calls.at(-1)
    expect(lastCall?.[1]).toBe('en')
  })

  test('subscribes to projection events and unsubscribes on unmount', () => {
    const { unmount } = render(<NavFull />)

    expect((window as any).projection.ipc.onEvent).toHaveBeenCalledTimes(1)
    expect(onEventCb).toBeDefined()

    const cb = onEventCb
    unmount()

    expect((window as any).projection.ipc.offEvent).toHaveBeenCalledWith(cb)
  })

  test('ignores non-navigation events', async () => {
    render(<NavFull />)

    await waitFor(() => {
      expect((window as any).projection.ipc.readNavigation).toHaveBeenCalled()
    })

    act(() => {
      onEventCb?.(null, { type: 'other', payload: { NaviStatus: 1 } })
    })

    expect(screen.queryByText('Turn right')).not.toBeInTheDocument()
  })

  test('updates state from navigation event payload wrapper', async () => {
    render(<NavFull />)

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
      expect(screen.getByText('Turn right')).toBeInTheDocument()
    })
  })

  test('updates state from navigation event with nested navi payload', async () => {
    render(<NavFull />)

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
      expect(screen.getByText('Turn right')).toBeInTheDocument()
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

    render(<NavFull />)

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
      expect(screen.getByText('Turn right')).toBeInTheDocument()
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

    render(<NavFull />)

    expect(await screen.findByAltText('Navigation maneuver')).toBeInTheDocument()
  })

  test('hides maneuver text when image exists and translated text is Unknown', async () => {
    translateNavigationMock.mockImplementation(() => ({
      ManeuverTypeText: 'Unknown',
      RemainDistanceText: '200 m',
      CurrentRoadName: 'Main Street',
      TimeRemainingToDestinationText: '12 min',
      DistanceRemainingDisplayStringText: '5 km',
      DestinationName: 'Berlin',
      SourceName: 'Maps',
      codes: {
        ManeuverType: 2,
        TurnSide: 2
      }
    }))
    ;(window as any).projection.ipc.readNavigation = jest.fn().mockResolvedValue({
      payload: {
        navi: {
          NaviStatus: 1,
          NaviImageBase64: 'abc123'
        }
      }
    })

    render(<NavFull />)

    expect(await screen.findByAltText('Navigation maneuver')).toBeInTheDocument()
    expect(screen.queryByText('Unknown')).not.toBeInTheDocument()
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

    render(<NavFull />)

    await waitFor(() => {
      expect(screen.getByText('Turn right')).toBeInTheDocument()
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

    expect(screen.getByText('Turn right')).toBeInTheDocument()
  })

  test('renders roundabout icon for roundabout maneuver types', async () => {
    translateNavigationMock.mockImplementation(() => ({
      ManeuverTypeText: 'Roundabout',
      RemainDistanceText: '150 m',
      CurrentRoadName: 'Ring',
      TimeRemainingToDestinationText: '8 min',
      DistanceRemainingDisplayStringText: '2 km',
      DestinationName: 'Center',
      SourceName: 'Maps',
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

    render(<NavFull />)

    await waitFor(() => {
      expect(screen.getByTestId('RoundaboutRightIcon')).toBeInTheDocument()
    })
  })

  test('renders right u-turn icon when maneuver type requires turn side handling', async () => {
    translateNavigationMock.mockImplementation(() => ({
      ManeuverTypeText: 'U-turn',
      RemainDistanceText: '80 m',
      CurrentRoadName: 'Main Street',
      TimeRemainingToDestinationText: '4 min',
      DistanceRemainingDisplayStringText: '1 km',
      DestinationName: 'Home',
      SourceName: 'Maps',
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

    render(<NavFull />)

    await waitFor(() => {
      expect(screen.getByTestId('UTurnRightIcon')).toBeInTheDocument()
    })
  })

  test('renders fallback help icon for unknown maneuver type', async () => {
    translateNavigationMock.mockImplementation(() => ({
      ManeuverTypeText: 'Unknown move',
      RemainDistanceText: '300 m',
      CurrentRoadName: 'Mystery Road',
      TimeRemainingToDestinationText: '6 min',
      DistanceRemainingDisplayStringText: '3 km',
      DestinationName: 'Somewhere',
      SourceName: 'Maps',
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

    render(<NavFull />)

    await waitFor(() => {
      expect(screen.getByTestId('HelpOutlinedIcon')).toBeInTheDocument()
    })
  })

  test('renders without optional metadata rows when translated values are missing', async () => {
    translateNavigationMock.mockImplementation(() => ({
      ManeuverTypeText: 'Continue',
      RemainDistanceText: '100 m',
      CurrentRoadName: '',
      TimeRemainingToDestinationText: '',
      DistanceRemainingDisplayStringText: '',
      DestinationName: '',
      SourceName: '',
      codes: {
        ManeuverType: 0,
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

    render(<NavFull />)

    await waitFor(() => {
      expect(screen.getByText('Continue')).toBeInTheDocument()
    })

    expect(screen.getByTestId('StraightIcon')).toBeInTheDocument()
    expect(screen.queryByText('Main Street')).not.toBeInTheDocument()
    expect(screen.queryByText('Berlin')).not.toBeInTheDocument()
    expect(screen.queryByText('Maps')).not.toBeInTheDocument()
  })
})
