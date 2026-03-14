import i18n from '../i18n'

describe('i18n setup', () => {
  test('initializes with english fallback and supports ua/uk aliases', () => {
    const fallback = i18n.options.fallbackLng
    expect(Array.isArray(fallback) ? fallback : [fallback]).toContain('en')
    expect(i18n.options.lng).toBe('en')
    expect(i18n.options.supportedLngs).toEqual(
      expect.arrayContaining(['en', 'de', 'ua', 'uk', 'uk-UA'])
    )
  })
})
