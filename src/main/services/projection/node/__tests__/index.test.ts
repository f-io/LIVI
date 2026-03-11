import ProjectionDefault, { DEFAULT_CONFIG, DongleDriver } from '@main/services/projection/node'
import ProjectionClass from '@main/services/projection/node/Projection'

describe('projection node index exports', () => {
  test('default export points to Projection class', () => {
    expect(ProjectionDefault).toBe(ProjectionClass)
  })

  test('re-exports config/types symbols', () => {
    expect(DEFAULT_CONFIG).toBeDefined()
    expect(DongleDriver).toBeDefined()
  })
})
