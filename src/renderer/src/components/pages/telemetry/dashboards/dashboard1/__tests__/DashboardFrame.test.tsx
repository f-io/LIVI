import { createTheme, ThemeProvider } from '@mui/material'
import { CarType } from '@shared/types'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { DashboardFrame } from '../DashboardFrame'

const useVehicleTelemetryMock = vi.fn()
const setClusterDashboardActive = vi.fn()
let settings: Record<string, unknown> | undefined

vi.mock('../../../hooks/useVehicleTelemetry', () => ({
  useVehicleTelemetry: () => useVehicleTelemetryMock()
}))

vi.mock('../../../components/DashboardShell', () => ({
  DashboardShell: ({ children }: { children: React.ReactNode }) => <div>{children}</div>
}))

vi.mock('@store/store', () => ({
  useLiviStore: (selector: (s: { settings: unknown }) => unknown) => selector({ settings }),
  useStatusStore: (selector: (s: { setClusterDashboardActive: unknown }) => unknown) =>
    selector({ setClusterDashboardActive })
}))

vi.mock('../../../widgets', () => ({
  GaugeArc: ({
    value,
    shadow,
    scaleMax,
    redline,
    labels
  }: {
    value: number
    shadow?: boolean
    scaleMax: number
    redline?: number
    labels: string[]
  }) => (
    <div
      data-scale={`${scaleMax}:${redline}:${labels.join(',')}`}
    >{`Gauge:${value}:${shadow}`}</div>
  ),
  labelOverflow: (gauge: { labels: string[] }) => (gauge.labels.length > 10 ? 12 : 0),
  FuelGauge: ({ level, mode }: { level: number; mode: string }) => (
    <div>{`Fuel:${mode}:${level}`}</div>
  ),
  TempGauge: ({ value, unit }: { value: number; unit: string }) => (
    <div>{`Temp:${value}:${unit}`}</div>
  ),
  SoftReadout: ({
    value,
    label,
    backdropColor
  }: {
    value: string | number
    label: string
    backdropColor?: string
  }) => <div>{`Soft:${label}:${value}:${backdropColor}`}</div>,
  normalizeGear: (g: string | number) => String(g),
  TelltaleBar: ({
    turn,
    hazards,
    lights,
    highBeam,
    parkingBrake,
    ambientC
  }: {
    turn: string
    hazards: boolean
    lights: boolean
    highBeam: boolean
    parkingBrake: boolean
    ambientC?: number
  }) => <div>{`Telltale:${turn}:${hazards}:${lights}:${highBeam}:${parkingBrake}:${ambientC}`}</div>
}))

describe('DashboardFrame', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    settings = { carType: undefined }

    useVehicleTelemetryMock.mockReturnValue({ telemetry: {} })
  })

  test('renders the full-cluster frame and toggles the cluster-dashboard flag', () => {
    settings = { carType: CarType.Electric }
    useVehicleTelemetryMock.mockReturnValue({
      telemetry: {
        speedKph: 88,
        rpm: 2200,
        gear: 'D',
        turnSignal: 'left',
        hazards: true,
        lights: true,
        highBeam: true,
        parkingBrake: true,
        ambientC: 15,
        fuelPct: 40,
        oilC: 95
      }
    })

    const { unmount } = render(<DashboardFrame clusterFull />)

    expect(setClusterDashboardActive).toHaveBeenCalledWith(true)
    expect(screen.getByText('Fuel:battery:40')).toBeInTheDocument()
    expect(screen.getByText('Gauge:88:true')).toBeInTheDocument()
    expect(screen.getByText('Soft:KPH:88:rgba(255, 255, 255, 0.78)')).toBeInTheDocument()
    expect(screen.getByText('Telltale:left:true:true:true:true:15')).toBeInTheDocument()

    unmount()

    expect(setClusterDashboardActive).toHaveBeenLastCalledWith(false)
  })

  test('renders the plain frame with fallbacks when telemetry is empty', () => {
    render(<DashboardFrame />)

    expect(setClusterDashboardActive).not.toHaveBeenCalled()
    expect(screen.getByText('Fuel:fuel:0')).toBeInTheDocument()
    expect(screen.getByText('Soft:KPH:0:undefined')).toBeInTheDocument()
    expect(screen.getAllByText('Gauge:0:undefined')).toHaveLength(2)
    expect(screen.getByText('Telltale:none:false:false:false:false:undefined')).toBeInTheDocument()
  })

  test('without settings the scales keep their defaults', () => {
    settings = undefined
    const { container } = render(<DashboardFrame />)

    const scales = Array.from(container.querySelectorAll('[data-scale]')).map((g) =>
      g.getAttribute('data-scale')
    )
    expect(scales).toEqual(['200:undefined:0,40,80,120,160,200', '5000:4500:0,1,2,3,4,5'])
    expect(screen.getByText('Temp:0:celsius')).toBeInTheDocument()
  })

  test('shows the car in its own units and limits', () => {
    settings = {
      speedUnit: 'mph',
      temperatureUnit: 'fahrenheit',
      maxSpeedKph: 260,
      speedScaleStep: 10,
      maxRpm: 8000,
      redlineRpm: 6500
    }
    useVehicleTelemetryMock.mockReturnValue({ telemetry: { speedKph: 100, oilC: 90 } })
    const { container } = render(<DashboardFrame />)

    expect(screen.getByText('Soft:MPH:62:undefined')).toBeInTheDocument()
    expect(screen.getByText('Temp:90:fahrenheit')).toBeInTheDocument()
    const scales = Array.from(container.querySelectorAll('[data-scale]')).map((g) =>
      g.getAttribute('data-scale')
    )
    expect(scales).toEqual([
      '150:undefined:0,10,20,30,40,50,60,70,80,90,100,110,120,130,140,150',
      '8000:6500:0,1,2,3,4,5,6,7,8'
    ])
    const shifts = Array.from(container.querySelectorAll('div'))
      .map((d) => window.getComputedStyle(d).transform)
      .filter((t) => t.endsWith('px)'))
    expect(shifts).toEqual(['translateX(12px)', 'translateX(-12px)'])
  })

  test('forwards a right turn signal', () => {
    useVehicleTelemetryMock.mockReturnValue({ telemetry: { turnSignal: 'right' } })

    render(<DashboardFrame />)

    expect(screen.getByText('Telltale:right:false:false:false:false:undefined')).toBeInTheDocument()
  })

  test('derives the stage scale from the window at first render', () => {
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 640 })
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 360 })
    try {
      const { container } = render(<DashboardFrame />)

      const stage = Array.from(container.querySelectorAll('div')).find((d) =>
        window.getComputedStyle(d).transform.includes('scale(0.5)')
      )
      expect(stage).toBeTruthy()
    } finally {
      Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1024 })
      Object.defineProperty(window, 'innerHeight', { configurable: true, value: 768 })
    }
  })

  test('rescales on a settled window resize, collapses blips and cancels on unmount', () => {
    vi.useFakeTimers()
    try {
      const { container, unmount } = render(<DashboardFrame />)

      Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1278 })
      Object.defineProperty(window, 'innerHeight', { configurable: true, value: 720 })
      fireEvent(window, new Event('resize'))
      act(() => {
        vi.advanceTimersByTime(50)
      })
      Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1280 })
      fireEvent(window, new Event('resize'))
      act(() => {
        vi.advanceTimersByTime(150)
      })

      const stage = Array.from(container.querySelectorAll('div')).find((d) =>
        window.getComputedStyle(d).transform.includes('scale(1)')
      )
      expect(stage).toBeTruthy()

      Object.defineProperty(window, 'innerWidth', { configurable: true, value: 0 })
      Object.defineProperty(window, 'innerHeight', { configurable: true, value: 0 })
      fireEvent(window, new Event('resize'))
      act(() => {
        vi.advanceTimersByTime(150)
      })

      const fallback = Array.from(container.querySelectorAll('div')).find((d) =>
        window.getComputedStyle(d).transform.includes('scale(1)')
      )
      expect(fallback).toBeTruthy()

      Object.defineProperty(window, 'innerWidth', { configurable: true, value: 640 })
      Object.defineProperty(window, 'innerHeight', { configurable: true, value: 360 })
      fireEvent(window, new Event('resize'))
      unmount()
      act(() => {
        vi.advanceTimersByTime(300)
      })
      expect(container.firstChild).toBeNull()
    } finally {
      Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1024 })
      Object.defineProperty(window, 'innerHeight', { configurable: true, value: 768 })
      vi.useRealTimers()
    }
  })

  test('renders under a dark theme', () => {
    render(
      <ThemeProvider theme={createTheme({ palette: { mode: 'dark' } })}>
        <DashboardFrame clusterFull />
      </ThemeProvider>
    )

    expect(screen.getAllByText('Gauge:0:true')).toHaveLength(2)
  })
})
