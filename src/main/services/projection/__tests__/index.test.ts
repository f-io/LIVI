import * as projection from '@main/services/projection'

describe('projection barrel exports', () => {
  test('exports expected runtime members', () => {
    expect(projection.DEFAULT_CONFIG).toBeDefined()
    expect(projection.DongleDriver).toBeDefined()

    expect(projection.Message).toBeDefined()
    expect(projection.MessageHeader).toBeDefined()
    expect(projection.MessageType).toBeDefined()

    expect(projection.HandDriveType).toBeDefined()
    expect(projection.PhoneWorkMode).toBeDefined()
    expect(projection.MicType).toBeDefined()
  })
})
