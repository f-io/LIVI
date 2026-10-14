import { EMPTY_GNSS_INFO, type GnssInfo } from '@shared/types'
import { render, screen } from '@testing-library/react'
import { GpsHwInfo } from '../GpsHwInfo'

const telemetry: { gnss?: GnssInfo } = {}

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }))
vi.mock('../../../../../telemetry/hooks/useVehicleTelemetry', () => ({
  useVehicleTelemetry: () => ({ telemetry, isStale: false })
}))

function show(gnss?: GnssInfo) {
  telemetry.gnss = gnss
  return render(<GpsHwInfo />)
}

describe('GpsHwInfo', () => {
  test('says so while nothing has been read yet', () => {
    show(undefined)
    expect(screen.getByText('settings.gpsNoHwInfo')).toBeInTheDocument()
  })

  test('renders the full module identity', () => {
    show({
      ...EMPTY_GNSS_INFO,
      version: {
        model: 'NEO-M9N',
        firmware: 'SPG 4.04',
        protocol: '32.01',
        software: 'ROM CORE 4.04 (d964f4)',
        hardware: '00190000',
        supported: ['GPS', 'GLO']
      }
    })
    expect(screen.getByText('NEO-M9N')).toBeInTheDocument()
    expect(screen.getByText('SPG 4.04')).toBeInTheDocument()
    expect(screen.getByText('32.01')).toBeInTheDocument()
    expect(screen.getByText('ROM CORE 4.04 (d964f4)')).toBeInTheDocument()
    expect(screen.getByText('00190000')).toBeInTheDocument()
    expect(screen.getByText('GPS, GLO')).toBeInTheDocument()
  })

  test('omits identity rows the receiver never reported', () => {
    show({ ...EMPTY_GNSS_INFO, version: { firmware: 'SPG 4.04' } })
    expect(screen.getByText('SPG 4.04')).toBeInTheDocument()
    expect(screen.queryByText('settings.gpsModel')).not.toBeInTheDocument()
  })

  test('renders the RF front end', () => {
    show({
      ...EMPTY_GNSS_INFO,
      rf: {
        antennaStatus: 'ok',
        antennaPower: 'on',
        jamming: 'ok',
        jammingIndicator: 12,
        agc: 4321,
        noise: 87
      }
    })
    expect(screen.getByText('settings.gpsAntennaOk')).toBeInTheDocument()
    expect(screen.getByText('settings.gpsAntennaPowerOn')).toBeInTheDocument()
    expect(screen.getByText('settings.gpsJammingOk · 12')).toBeInTheDocument()
    expect(screen.getByText('4321')).toBeInTheDocument()
    expect(screen.getByText('87')).toBeInTheDocument()
  })

  test.each([
    ['open', 'settings.gpsAntennaOpen'],
    ['short', 'settings.gpsAntennaShort'],
    ['init', 'settings.gpsAntennaInit'],
    ['unknown', 'settings.gpsAntennaUnknown']
  ])('names the antenna state %s', (status, key) => {
    show({
      ...EMPTY_GNSS_INFO,
      rf: {
        antennaStatus: status as never,
        antennaPower: 'off',
        jamming: 'warning',
        jammingIndicator: 200,
        agc: 8000,
        noise: 200
      }
    })
    expect(screen.getByText(key)).toBeInTheDocument()
    expect(screen.getByText('settings.gpsAntennaPowerOff')).toBeInTheDocument()
  })

  test('flags critical interference', () => {
    show({
      ...EMPTY_GNSS_INFO,
      rf: {
        antennaStatus: 'ok',
        antennaPower: 'unknown',
        jamming: 'critical',
        jammingIndicator: 255,
        agc: 100,
        noise: 900
      }
    })
    expect(screen.getByText('settings.gpsJammingCritical · 255')).toBeInTheDocument()
    expect(screen.getByText('settings.gpsAntennaUnknown')).toBeInTheDocument()
  })
})
