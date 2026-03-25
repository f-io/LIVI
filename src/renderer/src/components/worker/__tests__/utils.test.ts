import { createAudioPlayerKey } from '../utils'
import { decodeTypeMap } from '@shared/types/AudioDecode'

describe('createAudioPlayerKey', () => {
  test('creates key from decode type format and audio type', () => {
    const decodeType = Number(Object.keys(decodeTypeMap)[0])
    const audioType = 7

    const format = decodeTypeMap[decodeType]
    const result = createAudioPlayerKey(decodeType, audioType)

    expect(result).toBe(`${format.frequency}_${format.channel}_${audioType}`)
  })

  test('creates different keys for different audio types', () => {
    const decodeType = Number(Object.keys(decodeTypeMap)[0])

    expect(createAudioPlayerKey(decodeType, 1)).not.toBe(createAudioPlayerKey(decodeType, 2))
  })
})
