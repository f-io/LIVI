import { validate } from '@main/config/validateConfig'

describe('validate', () => {
  test('fills missing values with defaults', () => {
    const schema = {
      width: 800,
      flags: { kiosk: true, language: 'en' },
      list: [] as string[]
    }

    const result = validate({}, schema)

    expect(result).toEqual(schema)
  })

  test('keeps values with matching types and falls back when types mismatch', () => {
    const schema = {
      width: 800,
      title: 'LIVI',
      flags: { kiosk: true },
      list: [] as string[]
    }

    const input = {
      width: 1024,
      title: 123,
      flags: { kiosk: false },
      list: 'not-array'
    }

    const result = validate(input, schema)

    expect(result).toEqual({
      width: 1024,
      title: 'LIVI',
      flags: { kiosk: false },
      list: []
    })
  })
})
