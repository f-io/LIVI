import { CONFIG_PATH } from '@main/config/paths'

describe('CONFIG_PATH', () => {
  test('points to config.json inside app userData', () => {
    expect(CONFIG_PATH).toBe('/tmp/config.json')
  })
})
