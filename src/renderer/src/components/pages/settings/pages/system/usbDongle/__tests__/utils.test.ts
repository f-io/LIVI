import { fmt, isDongleFwCheckResponse, normalizeBoxInfo } from '../utils'

describe('usbDongle utils', () => {
  test('normalizeBoxInfo handles object/json/invalid values', () => {
    expect(normalizeBoxInfo({ uuid: 'x' })).toEqual({ uuid: 'x' })
    expect(normalizeBoxInfo('{"uuid":"x"}')).toEqual({ uuid: 'x' })
    expect(normalizeBoxInfo('bad-json')).toBeNull()
    expect(normalizeBoxInfo('   ')).toBeNull()
    expect(normalizeBoxInfo(null)).toBeNull()
  })

  test('fmt trims strings and returns null for empty values', () => {
    expect(fmt(' test ')).toBe('test')
    expect(fmt(42)).toBe('42')
    expect(fmt('   ')).toBeNull()
    expect(fmt(null)).toBeNull()
  })

  test('isDongleFwCheckResponse validates expected shape', () => {
    expect(isDongleFwCheckResponse({ ok: true, raw: { err: 0 } })).toBe(true)
    expect(isDongleFwCheckResponse({ ok: 'true', raw: { err: 0 } })).toBe(false)
    expect(isDongleFwCheckResponse({ ok: true, raw: null })).toBe(false)
    expect(isDongleFwCheckResponse(null)).toBe(false)
  })
})
