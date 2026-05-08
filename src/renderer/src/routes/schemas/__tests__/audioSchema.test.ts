import { audioSchema } from '../audioSchema'

const schema = audioSchema as any

describe('audioSchema', () => {
  test('exposes expected route structure and audio controls', () => {
    expect(schema.type).toBe('route')
    expect(schema.route).toBe('audio')
    expect(schema.label).toBe('Audio')
    expect(schema.labelKey).toBe('settings.audio')
    expect(schema.path).toBe('')
    expect(Array.isArray(schema.children)).toBe(true)
    expect(schema.children).toHaveLength(7)
  })

  test('music slider uses percent transform with sane defaults', () => {
    const node = schema.children[0]
    expect(node.type).toBe('slider')
    expect(node.path).toBe('audioVolume')
    expect(node.displayValue).toBe(true)
    expect(node.displayValueUnit).toBe('%')

    const vt = node.valueTransform!
    expect(vt.toView?.(undefined)).toBe(100)
    expect(vt.toView?.(0.456)).toBe(46)
    expect(vt.fromView?.(55, undefined)).toBe(0.55)
    expect(vt.fromView?.(Number.NaN, 0.7)).toBe(0.7)
    expect(vt.fromView?.(Number.NaN, undefined)).toBe(1)
    expect(vt.format?.(42)).toBe('42 %')
  })

  test('navigation, voiceAssistant and call sliders point to expected config paths', () => {
    expect(schema.children[1]).toEqual(
      expect.objectContaining({
        type: 'slider',
        path: 'navVolume',
        label: 'Navigation'
      })
    )

    expect(schema.children[2]).toEqual(
      expect.objectContaining({
        type: 'slider',
        path: 'voiceAssistantVolume',
        label: 'Voice Assistant'
      })
    )

    expect(schema.children[3]).toEqual(
      expect.objectContaining({
        type: 'slider',
        path: 'callVolume',
        label: 'Phone Calls'
      })
    )
  })

  test('microphone select exposes all expected options', () => {
    const node = schema.children[4]
    expect(node.type).toBe('select')
    expect(node.path).toBe('micType')
    expect(node.displayValue).toBe(true)
    expect(node.options).toEqual([
      { label: 'Car mic', labelKey: 'settings.micCar', value: 0 },
      { label: 'Dongle mic', labelKey: 'settings.micDongle', value: 1 },
      { label: 'Phone mic', labelKey: 'settings.micPhone', value: 2 }
    ])
  })

  test('sampling frequency select exposes expected options', () => {
    const sampling = schema.children[5]
    expect(sampling.type).toBe('select')
    expect(sampling.path).toBe('mediaSound')
    expect(sampling.options).toEqual([
      { label: '44.1 kHz', value: 0 },
      { label: '48 kHz', value: 1 }
    ])
  })

  test('disable audio checkbox is present as final leaf', () => {
    const node = schema.children[6]
    expect(node).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        label: 'Disable Audio',
        labelKey: 'settings.disableAudio',
        path: 'disableAudioOutput'
      })
    )
  })
})
