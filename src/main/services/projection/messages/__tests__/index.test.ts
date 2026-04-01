import * as messages from '@main/services/projection/messages'

describe('messages barrel export', () => {
  test('exports expected members', () => {
    expect(messages.Message).toBeDefined()
    expect(messages.MessageHeader).toBeDefined()
    expect(messages.decodeTypeMap).toBeDefined()
  })
})
