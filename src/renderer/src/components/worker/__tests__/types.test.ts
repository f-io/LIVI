import { isValidCommand } from '../types'

describe('worker types', () => {
  test('returns true for all valid commands', () => {
    const validCommands = [
      'left',
      'right',
      'next',
      'invalid',
      'pause',
      'play',
      'playPause',
      'selectDown',
      'back',
      'down',
      'home',
      'prev',
      'up',
      'selectUp',
      'acceptPhone',
      'rejectPhone',
      'siri',
      'frame',
      'mic',
      'deviceFound',
      'startRecordAudio',
      'stopRecordAudio',
      'requestHostUI',
      'wifiPair'
    ]

    for (const cmd of validCommands) {
      expect(isValidCommand(cmd)).toBe(true)
    }
  })

  test('returns false for invalid commands', () => {
    const invalidCommands = [
      '',
      'LEFT',
      'foo',
      'playpause',
      ' selectDown',
      'wifi_pair',
      'device-found'
    ]

    for (const cmd of invalidCommands) {
      expect(isValidCommand(cmd)).toBe(false)
    }
  })
})
