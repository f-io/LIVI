import { renderHook } from '@testing-library/react'
import { useSmartSettingsFromSchema } from '../useSmartSettingsFromSchema'

const smartResult = {
  state: {},
  isDirty: false,
  needsRestart: false,
  isDongleConnected: true,
  handleFieldChange: jest.fn(),
  resetState: jest.fn(),
  restart: jest.fn(),
  requestRestart: jest.fn()
}

jest.mock('../useSmartSettings', () => ({
  useSmartSettings: jest.fn(() => smartResult)
}))

describe('useSmartSettingsFromSchema', () => {
  test('flattens schema state and forwards requestRestart', () => {
    const schema = {
      type: 'route',
      route: 'settings',
      path: '',
      label: 'Settings',
      children: [
        { type: 'number', path: 'video.width', label: 'Width' },
        { type: 'checkbox', path: 'audio.mute', label: 'Mute', transform: (v: unknown) => !!v }
      ]
    } as any

    const settings = { video: { width: 800 }, audio: { mute: false } } as any
    const { result } = renderHook(() => useSmartSettingsFromSchema(schema, settings))

    result.current.requestRestart('video.width')
    expect(smartResult.requestRestart).toHaveBeenCalledWith('video.width')
    expect(result.current.state).toEqual({})
  })
})
