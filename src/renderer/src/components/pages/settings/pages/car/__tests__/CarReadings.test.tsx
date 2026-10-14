import { CarType, DEFAULT_CONFIG, EvConnectorType, type TelemetrySnapshot } from '@shared/types'
import { render, screen } from '@testing-library/react'
import { useLiviStore } from '../../../../../../store/store'
import { CarReadings } from '../CarReadings'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k })
}))

const show = (
  telemetry: TelemetrySnapshot | null,
  settings: Partial<typeof DEFAULT_CONFIG> = {}
) => {
  useLiviStore.setState({ telemetry, settings: { ...DEFAULT_CONFIG, ...settings } })
  return render(<CarReadings />)
}

afterEach(() => {
  useLiviStore.setState({ telemetry: null, settings: null })
})

describe('CarReadings', () => {
  test('shows nothing before the car has said anything', () => {
    const { container } = show(null)
    expect(container).toBeEmptyDOMElement()
  })

  test('a hybrid shows its capacity and the charge from the battery level', () => {
    show({ batteryCapacityKwh: 17.94, batteryLevelKwh: 9, fuelPct: 80 })
    expect(screen.getByText('17.9 kWh')).toBeInTheDocument()
    expect(screen.getByText('50 %')).toBeInTheDocument()
  })

  test('an electric car shows its fuel level as the charge', () => {
    show({ fuelPct: 64.4, batteryCapacityKwh: 0 }, { carType: CarType.Electric })
    expect(screen.getByText('settings.stateOfCharge')).toBeInTheDocument()
    expect(screen.getByText('64 %')).toBeInTheDocument()
  })

  test('a combustion car has no charge and broken values stay hidden', () => {
    const { container } = show({ fuelPct: 50, batteryCapacityKwh: Number.NaN })
    expect(container).toBeEmptyDOMElement()
  })

  test('the connectors show by name once there are some', () => {
    show(null, { evConnectorTypes: [EvConnectorType.Combo2, EvConnectorType.Mennekes] })
    expect(screen.getByText('Combo2, Mennekes')).toBeInTheDocument()
  })
})
