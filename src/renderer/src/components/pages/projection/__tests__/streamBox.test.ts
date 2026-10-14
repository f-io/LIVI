import { streamBox } from '../streamBox'

describe('streamBox', () => {
  test('takes the stream shape, as big as the window allows, centred', () => {
    expect(streamBox(1920, 1080)).toEqual({
      position: 'absolute',
      left: '50%',
      top: '50%',
      transform: 'translate(-50%, -50%)',
      width: 'min(100cqw, calc(100cqh * 1920 / 1080))',
      height: 'min(100cqh, calc(100cqw * 1080 / 1920))'
    })
  })

  test('fills the window while the stream size is unknown', () => {
    for (const [w, h] of [
      [0, 1080],
      [1920, 0],
      [undefined, undefined]
    ] as const) {
      const box = streamBox(w as number, h as number)
      expect([box.width, box.height]).toEqual(['100%', '100%'])
      expect(box.transform).toBe('translate(-50%, -50%)')
    }
  })
})
