describe('main constants', () => {
  const originalDebug = process.env.DEBUG

  beforeEach(() => {
    jest.resetModules()
    process.env.DEBUG = originalDebug
  })

  afterAll(() => {
    process.env.DEBUG = originalDebug
  })

  test('exports expected window size constants', () => {
    const constants = require('../constants')

    expect(constants.MIN_WIDTH).toBe(300)
    expect(constants.MIN_HEIGHT).toBe(200)
    expect(constants.DEFAULT_WIDTH).toBe(800)
    expect(constants.DEFAULT_HEIGHT).toBe(480)
  })

  test('NULL_DELETES contains expected resettable config keys', () => {
    const { NULL_DELETES } = require('../constants')

    expect(NULL_DELETES).toEqual([
      'primaryColorDark',
      'primaryColorLight',
      'highlightColorDark',
      'highlightColorLight'
    ])
  })

  test('DEBUG is true when DEBUG=1', () => {
    process.env.DEBUG = '1'
    jest.resetModules()

    const { DEBUG } = require('../constants')

    expect(DEBUG).toBe(true)
  })

  test('DEBUG is false when DEBUG is not 1', () => {
    process.env.DEBUG = '0'
    jest.resetModules()

    const { DEBUG } = require('../constants')

    expect(DEBUG).toBe(false)
  })

  test('mimeTypeFromExt returns known mime types case-insensitively', () => {
    const { mimeTypeFromExt } = require('../constants')

    expect(mimeTypeFromExt('.html')).toBe('text/html')
    expect(mimeTypeFromExt('.JS')).toBe('text/javascript')
    expect(mimeTypeFromExt('.Jpeg')).toBe('image/jpeg')
    expect(mimeTypeFromExt('.SVG')).toBe('image/svg+xml')
    expect(mimeTypeFromExt('.wasm')).toBe('application/wasm')
  })

  test('mimeTypeFromExt falls back to application/octet-stream for unknown extensions', () => {
    const { mimeTypeFromExt } = require('../constants')

    expect(mimeTypeFromExt('.bin')).toBe('application/octet-stream')
    expect(mimeTypeFromExt('.unknown')).toBe('application/octet-stream')
  })
})
