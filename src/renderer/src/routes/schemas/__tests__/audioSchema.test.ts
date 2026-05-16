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
    expect(schema.children).toHaveLength(8)
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

  test('audio output device is a select with loadOptions + page', () => {
    const out = schema.children[4]
    expect(out).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'audioOutputDevice',
        label: 'Audio Output',
        displayValue: true
      })
    )
    expect(typeof out.loadOptions).toBe('function')
    expect(out.options[0]).toEqual(
      expect.objectContaining({
        value: '',
        labelKey: 'settings.audioDeviceSystemDefault'
      })
    )
    expect(out.page).toEqual(
      expect.objectContaining({ title: 'Audio Output', labelTitle: 'settings.audioOutputDevice' })
    )
  })

  test('audio input device is a select with loadOptions + page', () => {
    const inp = schema.children[5]
    expect(inp).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'audioInputDevice',
        label: 'Audio Input',
        displayValue: true
      })
    )
    expect(typeof inp.loadOptions).toBe('function')
  })

  test('sampling frequency select sits between audio input and disable audio', () => {
    const node = schema.children[6]
    expect(node).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'samplingFrequency',
        labelKey: 'settings.samplingFrequency'
      })
    )
    expect(node.options).toEqual([
      { label: '44.1 kHz', value: 0 },
      { label: '48 kHz', value: 1 }
    ])
  })

  test('disable audio checkbox is present as final leaf', () => {
    const node = schema.children[7]
    expect(node).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        label: 'Disable Audio',
        labelKey: 'settings.disableAudio',
        path: 'disableAudioOutput'
      })
    )
  })

  test('does not contain Microphone (lives in dongle firmware route)', () => {
    const paths = schema.children.map((c: { path?: string }) => c.path)
    expect(paths).not.toContain('micType')
  })
})
