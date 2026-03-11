import * as sharedUtils from '@main/shared/utils'

describe('main/shared/utils index exports', () => {
  test('exports translateNavigation', () => {
    expect(typeof sharedUtils.translateNavigation).toBe('function')
  })
})
