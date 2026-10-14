import { CarType, type Config } from '@shared/types'
import type { NumberNode, SelectNode } from '../../types'
import { carSchema } from '../carSchema'

const children = (carSchema as { children: { path: string; type: string }[] }).children
const node = <T>(path: string) => children.find((c) => c.path === path && c.type !== 'custom') as T

describe('carSchema', () => {
  test('is its own section with the car settings and the readings last', () => {
    expect(carSchema).toEqual(
      expect.objectContaining({ type: 'route', route: 'car', labelKey: 'settings.car' })
    )
    expect(children.map((c) => c.path)).toEqual([
      'carType',
      'hand',
      'maxSpeedKph',
      'speedScaleStep',
      'maxRpm',
      'redlineRpm',
      'speedUnit',
      'temperatureUnit',
      'carPlayVehicleClimate',
      'carPlayDockPosition',
      'carType'
    ])
    expect(children.at(-1)?.type).toBe('custom')
  })

  test('every car type has a translated option', () => {
    const options = node<SelectNode>('carType').options
    expect(options[0]).toEqual({
      label: 'Gasoline',
      labelKey: 'settings.carTypeGasoline',
      value: CarType.Gasoline
    })
    expect(options.map((o) => o.value)).toContain(CarType.HybridDiesel)
    expect(options.map((o) => o.value)).not.toContain(CarType.Unknown)
  })

  test('the max speed moves in scale steps of the chosen speed unit', () => {
    const unit = node<NumberNode>('maxSpeedKph').unit
    expect(unit?.(null)).toEqual({ factor: 1, suffix: 'km/h', step: 40, max: 600 })
    const mph = unit?.({ speedUnit: 'mph', speedScaleStep: 10 } as Config)
    expect([mph?.suffix, mph?.step, mph?.max]).toEqual(['mph', 10, 150])
    expect(node<SelectNode>('speedScaleStep').options.map((o) => o.value)).toEqual([10, 20, 40])
    expect(200 * (mph?.factor ?? 0)).toBeCloseTo(124.27, 2)
  })
})
