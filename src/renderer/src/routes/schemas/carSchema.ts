import { KM_PER_MILE } from '@renderer/utils/units'
import { CarType, type Config, DEFAULT_CONFIG, HandDriveType } from '@shared/types'
import { CarReadings } from '../../components/pages/settings/pages/car/CarReadings'
import { SPEED_MAX_SECTIONS } from '../../components/pages/telemetry/dashboards/gaugeScale'
import type { NumberUnit, SettingsNode } from '../types'

// The scale ends on a whole step, so the value moves in those.
const speedScale = (settings: Config | null): NumberUnit => {
  const step = settings?.speedScaleStep ?? DEFAULT_CONFIG.speedScaleStep
  const mph = settings?.speedUnit === 'mph'
  return {
    factor: mph ? 1 / KM_PER_MILE : 1,
    suffix: mph ? 'mph' : 'km/h',
    step,
    max: SPEED_MAX_SECTIONS * step
  }
}

const CAR_TYPES: [string, CarType][] = [
  ['Gasoline', CarType.Gasoline],
  ['Diesel', CarType.Diesel],
  ['Electric', CarType.Electric],
  ['HybridGasoline', CarType.HybridGasoline],
  ['HybridDiesel', CarType.HybridDiesel],
  ['E85', CarType.E85],
  ['Biodiesel', CarType.Biodiesel],
  ['DieselWinter', CarType.DieselWinter],
  ['LPG', CarType.LPG],
  ['CNG', CarType.CNG],
  ['LNG', CarType.LNG],
  ['Hydrogen', CarType.Hydrogen],
  ['Other', CarType.Other]
]

export const carSchema: SettingsNode<Config> = {
  type: 'route',
  route: 'car',
  label: 'Car',
  labelKey: 'settings.car',
  icon: 'car',
  path: '',
  children: [
    {
      type: 'select',
      label: 'Fuel Type',
      labelKey: 'settings.carType',
      icon: 'carType',
      path: 'carType',
      displayValue: true,
      options: CAR_TYPES.map(([name, value]) => ({
        label: name,
        labelKey: `settings.carType${name}`,
        value
      })),
      page: {
        title: 'Fuel Type',
        labelTitle: 'settings.carType'
      }
    },
    {
      type: 'select',
      label: 'Steering wheel position',
      labelKey: 'settings.steeringWheelPosition',
      icon: 'steering',
      path: 'hand',
      displayValue: true,
      options: [
        { label: 'LHD', labelKey: 'settings.lhdr', value: HandDriveType.LHD },
        { label: 'RHD', labelKey: 'settings.rhdr', value: HandDriveType.RHD }
      ],
      page: {
        title: 'Steering wheel position',
        labelTitle: 'settings.steeringWheelPosition'
      }
    },
    {
      type: 'number',
      label: 'Max Speed',
      labelKey: 'settings.maxSpeed',
      icon: 'maxSpeed',
      path: 'maxSpeedKph',
      min: 60,
      max: 400,
      unit: speedScale,
      displayValue: true,
      page: {
        title: 'Max Speed',
        labelTitle: 'settings.maxSpeed'
      }
    },
    {
      type: 'select',
      label: 'Scale Step',
      labelKey: 'settings.speedScaleStep',
      icon: 'speedScaleStep',
      path: 'speedScaleStep',
      displayValue: true,
      options: [10, 20, 40].map((value) => ({ label: String(value), value })),
      page: {
        title: 'Scale Step',
        labelTitle: 'settings.speedScaleStep'
      }
    },
    {
      type: 'number',
      label: 'Max RPM',
      labelKey: 'settings.maxRpm',
      icon: 'maxRpm',
      path: 'maxRpm',
      min: 3000,
      max: 20000,
      step: 1000,
      displayValue: true,
      displayValueUnit: ' rpm',
      page: {
        title: 'Max RPM',
        labelTitle: 'settings.maxRpm'
      }
    },
    {
      type: 'number',
      label: 'Redline',
      labelKey: 'settings.redlineRpm',
      icon: 'redlineRpm',
      path: 'redlineRpm',
      min: 0,
      max: 20000,
      step: 100,
      displayValue: true,
      displayValueUnit: ' rpm',
      page: {
        title: 'Redline',
        labelTitle: 'settings.redlineRpm'
      }
    },
    {
      type: 'select',
      label: 'Speed Unit',
      labelKey: 'settings.speedUnit',
      icon: 'speedUnit',
      path: 'speedUnit',
      displayValue: true,
      options: [
        { label: 'km/h', value: 'kmh' },
        { label: 'mph', value: 'mph' }
      ],
      page: {
        title: 'Speed Unit',
        labelTitle: 'settings.speedUnit'
      }
    },
    {
      type: 'select',
      label: 'Temperature Unit',
      labelKey: 'settings.temperatureUnit',
      icon: 'temperatureUnit',
      path: 'temperatureUnit',
      displayValue: true,
      options: [
        { label: '°C', value: 'celsius' },
        { label: '°F', value: 'fahrenheit' }
      ],
      page: {
        title: 'Temperature Unit',
        labelTitle: 'settings.temperatureUnit'
      }
    },
    {
      type: 'checkbox',
      label: 'CarPlay Climate Controls',
      labelKey: 'settings.carPlayVehicleClimate',
      icon: 'climate',
      path: 'carPlayVehicleClimate'
    },
    {
      type: 'select',
      label: 'CarPlay Dock',
      labelKey: 'settings.carPlayDockPosition',
      icon: 'edgeBottom',
      path: 'carPlayDockPosition',
      displayValue: true,
      options: [
        { label: 'Bottom', labelKey: 'settings.carPlayDockBottom', value: 'bottom' },
        { label: 'Side', labelKey: 'settings.carPlayDockSide', value: 'side' },
        { label: 'Automatic', labelKey: 'settings.carPlayDockAuto', value: 'auto' }
      ],
      page: {
        title: 'CarPlay Dock',
        labelTitle: 'settings.carPlayDockPosition'
      }
    },
    {
      type: 'custom',
      label: 'Car Data',
      labelKey: 'settings.carData',
      path: 'carType',
      component: CarReadings
    }
  ]
}
