import { loadImageFromFile, resizeImageToBase64Png } from '../utils'

describe('iconUploader utils', () => {
  describe('loadImageFromFile', () => {
    const originalFileReader = global.FileReader
    const originalImage = global.Image

    afterEach(() => {
      global.FileReader = originalFileReader
      global.Image = originalImage
      jest.restoreAllMocks()
    })

    test('resolves with image after file is read successfully', async () => {
      class MockFileReader {
        static lastInstance: MockFileReader | null = null

        onload: null | (() => void) = null
        onerror: null | ((err: unknown) => void) = null
        result: string | null = null

        constructor() {
          MockFileReader.lastInstance = this
        }

        readAsDataURL() {
          this.result = 'data:image/png;base64,abc123'
          this.onload?.()
        }
      }

      class MockImage {
        onload: null | (() => void) = null
        onerror: null | ((err: unknown) => void) = null

        set src(_value: string) {
          this.onload?.()
        }
      }

      global.FileReader = MockFileReader as any
      global.Image = MockImage as any

      const file = new File(['x'], 'icon.png', { type: 'image/png' })
      const result = await loadImageFromFile(file)

      expect(MockFileReader.lastInstance?.result).toBe('data:image/png;base64,abc123')
      expect(result).toBeInstanceOf(MockImage)
    })

    test('rejects when FileReader errors', async () => {
      class MockFileReader {
        onload: null | (() => void) = null
        onerror: null | ((err: unknown) => void) = null
        result: string | null = null

        readAsDataURL() {
          this.onerror?.(new Error('read failed'))
        }
      }

      class MockImage {}

      global.FileReader = MockFileReader as any
      global.Image = MockImage as any

      const file = new File(['x'], 'icon.png', { type: 'image/png' })

      await expect(loadImageFromFile(file)).rejects.toThrow('read failed')
    })

    test('rejects when image loading fails after FileReader succeeds', async () => {
      class MockFileReader {
        onload: null | (() => void) = null
        onerror: null | ((err: unknown) => void) = null
        result: string | null = null

        readAsDataURL() {
          this.result = 'data:image/png;base64,abc123'
          this.onload?.()
        }
      }

      class MockImage {
        onload: null | (() => void) = null
        onerror: null | ((err: unknown) => void) = null

        set src(_value: string) {
          this.onerror?.(new Error('image failed'))
        }
      }

      global.FileReader = MockFileReader as any
      global.Image = MockImage as any

      const file = new File(['x'], 'icon.png', { type: 'image/png' })

      await expect(loadImageFromFile(file)).rejects.toThrow('image failed')
    })
  })

  describe('resizeImageToBase64Png', () => {
    afterEach(() => {
      jest.restoreAllMocks()
    })

    test('resizes image and returns base64 png without prefix', () => {
      const clearRect = jest.fn()
      const drawImage = jest.fn()

      const toDataURL = jest.fn(() => 'data:image/png;base64,encoded-png-data')
      const getContext = jest.fn(() => ({
        clearRect,
        drawImage
      }))

      jest.spyOn(document, 'createElement').mockImplementation(((tagName: string) => {
        if (tagName === 'canvas') {
          return {
            width: 0,
            height: 0,
            getContext,
            toDataURL
          } as any
        }

        return document.createElement(tagName)
      }) as typeof document.createElement)

      const img = {
        width: 200,
        height: 100
      } as HTMLImageElement

      const result = resizeImageToBase64Png(img, 64)

      expect(getContext).toHaveBeenCalledWith('2d')
      expect(clearRect).toHaveBeenCalledWith(0, 0, 64, 64)
      expect(drawImage).toHaveBeenCalledWith(img, -32, 0, 128, 64)
      expect(toDataURL).toHaveBeenCalledWith('image/png')
      expect(result).toBe('encoded-png-data')
    })

    test('throws when canvas 2d context is unavailable', () => {
      const getContext = jest.fn(() => null)

      jest.spyOn(document, 'createElement').mockImplementation(((tagName: string) => {
        if (tagName === 'canvas') {
          return {
            width: 0,
            height: 0,
            getContext
          } as any
        }

        return document.createElement(tagName)
      }) as typeof document.createElement)

      const img = {
        width: 100,
        height: 100
      } as HTMLImageElement

      expect(() => resizeImageToBase64Png(img, 64)).toThrow('Canvas 2D context not available')
    })
  })
})
