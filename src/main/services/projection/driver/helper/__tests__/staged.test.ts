import { clearHelperRestaged, helperRestaged, markHelperRestaged } from '../staged'

describe('staged', () => {
  // One flag for the whole run, so the steps belong in one test.
  test('starts unset, mark raises it, clear takes it back', () => {
    expect(helperRestaged()).toBe(false)
    markHelperRestaged()
    expect(helperRestaged()).toBe(true)
    clearHelperRestaged()
    expect(helperRestaged()).toBe(false)
  })
})
