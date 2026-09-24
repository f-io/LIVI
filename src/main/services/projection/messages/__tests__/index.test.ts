import * as messages from '@main/services/projection/messages'

describe('messages barrel export', () => {
  test('exports expected members', () => {
    expect(messages.Message).toBeDefined()
    expect(messages.SendableMessage).toBeDefined()
    expect(messages.DEFAULT_CONFIG).toBeDefined()
    expect(messages.decodeTypeMap).toBeDefined()
  })
})
