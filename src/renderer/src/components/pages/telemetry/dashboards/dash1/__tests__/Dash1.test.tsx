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
  GaugeArc: ({ value }: { value: number }) => <div>Gauge:{value}</div>,
  FuelGauge: ({ level, mode }: { level: number; mode: string }) => (
    <div>
      Fuel:{mode}:{level}
    </div>
  ),
  TempGauge: ({ value }: { value: number }) => <div>Temp:{value}</div>,
  NavMini: ({ iconSize }: { iconSize: number }) => <div>NavMini:{iconSize}</div>,
  SoftReadout: ({ value, label }: { value: string | number; label: string }) => (
    <div>
      Soft:{label}:{String(value)}
    </div>
  ),
  normalizeGear: (g: string | number) => String(g),
  TelltaleBar: ({ turn, hazards }: { turn: string; hazards: boolean }) => (
    <div>
      Telltale:{turn}:{String(hazards)}
    </div>
  )
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

    // left ring is fed speed, right ring is fed rpm
    expect(screen.getByText('Gauge:123')).toBeInTheDocument()
    expect(screen.getByText('Gauge:3456')).toBeInTheDocument()
    // soft readouts: speed in the left ring, gear in the right ring
    expect(screen.getByText('Soft:KPH:123')).toBeInTheDocument()
    expect(screen.getByText('Soft:GEAR:D')).toBeInTheDocument()
    expect(screen.getByText('NavMini:84')).toBeInTheDocument()
    expect(screen.getByText('Telltale:none:false')).toBeInTheDocument()
  })

  test('falls back to default values when telemetry fields are missing', () => {
    useVehicleTelemetryMock.mockReturnValue({
      telemetry: {}
    })

    render(<Dash1 />)

    // both rings read 0 (speed + rpm)
    expect(screen.getAllByText('Gauge:0')).toHaveLength(2)
    expect(screen.getByText('Soft:KPH:0')).toBeInTheDocument()
    expect(screen.getByText('Soft:GEAR:P')).toBeInTheDocument()
  })

  test('accepts numeric gear value', () => {
    useVehicleTelemetryMock.mockReturnValue({
      telemetry: {
        gear: 3
      }
    })

    render(<Dash1 />)

    expect(screen.getByText('Soft:GEAR:3')).toBeInTheDocument()
  })

  test('observes host element with ResizeObserver', () => {
    render(<Dash1 />)

    expect(observeMock).toHaveBeenCalledTimes(1)
  })

  test('handles ResizeObserver update with valid size without breaking rendered widgets', async () => {
    render(<Dash1 />)

    resizeObserverCallback?.([{ contentRect: { width: 640, height: 360 } }])

    await waitFor(() => {
      expect(screen.getByText('Soft:KPH:123')).toBeInTheDocument()
      expect(screen.getByText('Gauge:3456')).toBeInTheDocument()
      expect(screen.getByText('NavMini:84')).toBeInTheDocument()
    })
  })

  test('falls back safely when ResizeObserver reports invalid size', async () => {
    render(<Dash1 />)

    resizeObserverCallback?.([{ contentRect: { width: 0, height: 0 } }])

    await waitFor(() => {
      expect(screen.getByText('Soft:KPH:123')).toBeInTheDocument()
      expect(screen.getByText('Gauge:3456')).toBeInTheDocument()
      expect(screen.getByText('NavMini:84')).toBeInTheDocument()
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
      expect(screen.getByText('Soft:KPH:123')).toBeInTheDocument()
      expect(screen.getByText('Gauge:3456')).toBeInTheDocument()
      expect(screen.getByText('NavMini:84')).toBeInTheDocument()
    })
  })
})
