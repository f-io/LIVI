import { render, screen, waitFor } from '@testing-library/react'
import { Dash1 } from '../Dash1'

const useVehicleTelemetryMock = jest.fn()

let resizeObserverCallback:
  | ((entries: Array<{ contentRect: { width: number; height: number } }>) => void)
  | null = null

const observeMock = jest.fn()
const disconnectMock = jest.fn()

jest.mock('../../../hooks/useVehicleTelemetry', () => ({
  useVehicleTelemetry: () => useVehicleTelemetryMock()
}))

jest.mock('../../../components/DashShell', () => ({
  DashShell: ({ children }: { children: React.ReactNode }) => <div>{children}</div>
}))

jest.mock('../../../widgets', () => ({
  Speed: ({ speedKph }: { speedKph: number }) => <div>Speed:{speedKph}</div>,
  Rpm: ({ rpm }: { rpm: number }) => <div>Rpm:{rpm}</div>,
  RpmRing: ({ rpm }: { rpm: number }) => <div>RpmRing:{rpm}</div>,
  Gear: ({ gear }: { gear: string | number }) => <div>Gear:{String(gear)}</div>,
  CoolantTemp: ({ coolantC }: { coolantC: number }) => <div>Coolant:{coolantC}</div>,
  OilTemp: ({ oilC }: { oilC: number }) => <div>Oil:{oilC}</div>,
  FuelLevel: ({ fuelPct }: { fuelPct: number }) => <div>Fuel:{fuelPct}</div>,
  NavMini: ({ iconSize }: { iconSize: number }) => <div>NavMini:{iconSize}</div>
}))

describe('Dash1', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    resizeObserverCallback = null
    ;(global as any).ResizeObserver = class {
      constructor(
        cb: (entries: Array<{ contentRect: { width: number; height: number } }>) => void
      ) {
        resizeObserverCallback = cb
      }

      observe = observeMock
      disconnect = disconnectMock
    }

    useVehicleTelemetryMock.mockReturnValue({
      telemetry: {
        speedKph: 123,
        rpm: 3456,
        coolantC: 91,
        oilC: 103,
        fuelPct: 67,
        gear: 'D'
      }
    })
  })

  test('renders all dashboard widgets with telemetry values', () => {
    render(<Dash1 />)

    expect(screen.getByText('Speed:123')).toBeInTheDocument()
    expect(screen.getByText('Rpm:3456')).toBeInTheDocument()
    expect(screen.getByText('RpmRing:3456')).toBeInTheDocument()
    expect(screen.getByText('Gear:D')).toBeInTheDocument()
    expect(screen.getByText('Coolant:91')).toBeInTheDocument()
    expect(screen.getByText('Oil:103')).toBeInTheDocument()
    expect(screen.getByText('Fuel:67')).toBeInTheDocument()
    expect(screen.getByText('NavMini:84')).toBeInTheDocument()
  })

  test('falls back to default values when telemetry fields are missing', () => {
    useVehicleTelemetryMock.mockReturnValue({
      telemetry: {}
    })

    render(<Dash1 />)

    expect(screen.getByText('Speed:0')).toBeInTheDocument()
    expect(screen.getByText('Rpm:0')).toBeInTheDocument()
    expect(screen.getByText('RpmRing:0')).toBeInTheDocument()
    expect(screen.getByText('Gear:P')).toBeInTheDocument()
    expect(screen.getByText('Coolant:0')).toBeInTheDocument()
    expect(screen.getByText('Oil:0')).toBeInTheDocument()
    expect(screen.getByText('Fuel:0')).toBeInTheDocument()
  })

  test('accepts numeric gear value', () => {
    useVehicleTelemetryMock.mockReturnValue({
      telemetry: {
        gear: 3
      }
    })

    render(<Dash1 />)

    expect(screen.getByText('Gear:3')).toBeInTheDocument()
  })

  test('observes host element with ResizeObserver', () => {
    render(<Dash1 />)

    expect(observeMock).toHaveBeenCalledTimes(1)
  })

  test('handles ResizeObserver update with valid size without breaking rendered widgets', async () => {
    render(<Dash1 />)

    resizeObserverCallback?.([{ contentRect: { width: 640, height: 360 } }])

    await waitFor(() => {
      expect(screen.getByText('Speed:123')).toBeInTheDocument()
      expect(screen.getByText('Rpm:3456')).toBeInTheDocument()
      expect(screen.getByText('NavMini:84')).toBeInTheDocument()
    })
  })

  test('falls back safely when ResizeObserver reports invalid size', async () => {
    render(<Dash1 />)

    resizeObserverCallback?.([{ contentRect: { width: 0, height: 0 } }])

    await waitFor(() => {
      expect(screen.getByText('Speed:123')).toBeInTheDocument()
      expect(screen.getByText('RpmRing:3456')).toBeInTheDocument()
      expect(screen.getByText('Fuel:67')).toBeInTheDocument()
    })
  })

  test('disconnects ResizeObserver on unmount', () => {
    const { unmount } = render(<Dash1 />)

    unmount()

    expect(disconnectMock).toHaveBeenCalledTimes(1)
  })

  test('ignores ResizeObserver entries without contentRect', async () => {
    render(<Dash1 />)

    resizeObserverCallback?.([{} as never])

    await waitFor(() => {
      expect(screen.getByText('Speed:123')).toBeInTheDocument()
      expect(screen.getByText('Rpm:3456')).toBeInTheDocument()
      expect(screen.getByText('Fuel:67')).toBeInTheDocument()
    })
  })
})
