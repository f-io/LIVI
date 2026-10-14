import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Dashboard1 } from '../Dashboard1'

const useVehicleTelemetryMock = vi.fn()

vi.mock('../../../hooks/useVehicleTelemetry', () => ({
  useVehicleTelemetry: () => useVehicleTelemetryMock()
}))

vi.mock('../../../components/DashboardShell', () => ({
  DashboardShell: ({ children }: { children: React.ReactNode }) => <div>{children}</div>
}))

vi.mock('../../../widgets', () => ({
  GaugeArc: ({ value }: { value: number }) => <div>Gauge:{value}</div>,
  labelOverflow: () => 0,
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

describe('Dashboard1', () => {
  beforeEach(async () => {
    vi.clearAllMocks()

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

  test('renders all dashboard widgets with telemetry values', async () => {
    render(<Dashboard1 />)

    // left ring is fed speed, right ring is fed rpm
    expect(screen.getByText('Gauge:123')).toBeInTheDocument()
    expect(screen.getByText('Gauge:3456')).toBeInTheDocument()
    // soft readouts: speed in the left ring, gear in the right ring
    expect(screen.getByText('Soft:KPH:123')).toBeInTheDocument()
    expect(screen.getByText('Soft:GEAR:D')).toBeInTheDocument()
    expect(screen.getByText('NavMini:84')).toBeInTheDocument()
    expect(screen.getByText('Telltale:none:false')).toBeInTheDocument()
  })

  test('falls back to default values when telemetry fields are missing', async () => {
    useVehicleTelemetryMock.mockReturnValue({
      telemetry: {}
    })

    render(<Dashboard1 />)

    // both rings read 0 (speed + rpm)
    expect(screen.getAllByText('Gauge:0')).toHaveLength(2)
    expect(screen.getByText('Soft:KPH:0')).toBeInTheDocument()
    expect(screen.getByText('Soft:GEAR:P')).toBeInTheDocument()
  })

  test('accepts numeric gear value', async () => {
    useVehicleTelemetryMock.mockReturnValue({
      telemetry: {
        gear: 3
      }
    })

    render(<Dashboard1 />)

    expect(screen.getByText('Soft:GEAR:3')).toBeInTheDocument()
  })

  test('handles a window resize without breaking rendered widgets', async () => {
    const { unmount } = render(<Dashboard1 />)

    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 640 })
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 360 })
    fireEvent(window, new Event('resize'))

    await waitFor(() => {
      expect(screen.getByText('Soft:KPH:123')).toBeInTheDocument()
      expect(screen.getByText('Gauge:3456')).toBeInTheDocument()
      expect(screen.getByText('NavMini:84')).toBeInTheDocument()
    })

    unmount()
  })
})
