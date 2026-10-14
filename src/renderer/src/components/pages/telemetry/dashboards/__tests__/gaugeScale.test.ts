import { gaugeScale, RPM_MAX_SECTIONS, RPM_STEP, SPEED_MAX_SECTIONS } from '../gaugeScale'

const speed = (max: number, step: number) => gaugeScale(max, step, SPEED_MAX_SECTIONS, String)
const rpm = (max: number) => gaugeScale(max, RPM_STEP, RPM_MAX_SECTIONS, (v) => String(v / 1000))

describe('gaugeScale', () => {
  test('the default keeps the scale the dashboard always had', () => {
    expect(speed(200, 40)).toEqual({
      scaleMax: 200,
      majorCount: 6,
      ticks: 41,
      labels: ['0', '40', '80', '120', '160', '200']
    })
    expect(rpm(5000).labels).toEqual(['0', '1', '2', '3', '4', '5'])
  })

  test('a number every step up to the set value', () => {
    const fine = speed(180, 20)
    expect([fine.scaleMax, fine.majorCount, fine.ticks]).toEqual([180, 10, 37])
    expect(speed(120, 10).labels.at(-1)).toBe('120')
  })

  test('a value off the step, like km/h shown in mph, lands on the nearest one', () => {
    expect(speed(124.27, 20).scaleMax).toBe(120)
    expect(speed(0, 40).scaleMax).toBe(40)
  })

  test('the numbers never get so many that they touch', () => {
    expect(speed(300, 10).scaleMax).toBe(150)
    expect(rpm(25000).scaleMax).toBe(20000)
  })
})
