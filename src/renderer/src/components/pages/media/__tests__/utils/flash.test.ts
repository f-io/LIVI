import { flash } from '../../utils/flash'
import { FLASH_TIMEOUT_MS } from '../../constants'

describe('flash', () => {
  beforeEach(() => {
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.runOnlyPendingTimers()
    jest.useRealTimers()
  })

  test('does nothing when ref has no current element', () => {
    const ref = {
      current: null
    }

    expect(() => flash(ref)).not.toThrow()
  })

  test('applies flash styles immediately and restores previous styles after default timeout', () => {
    const button = document.createElement('button')
    button.style.transform = 'scale(1)'
    button.style.boxShadow = 'none'

    const ref = {
      current: button
    }

    flash(ref)

    expect(button.style.transform).toBe('scale(0.94)')
    expect(button.style.boxShadow).toBe('0 0 0 5px rgba(255,255,255,0.35) inset')

    jest.advanceTimersByTime(FLASH_TIMEOUT_MS)

    expect(button.style.transform).toBe('scale(1)')
    expect(button.style.boxShadow).toBe('none')
  })

  test('restores previous styles after a custom timeout', () => {
    const button = document.createElement('button')
    button.style.transform = 'translateX(2px)'
    button.style.boxShadow = 'rgb(1, 2, 3) 0px 0px 1px inset'

    const ref = {
      current: button
    }

    flash(ref, 123)

    expect(button.style.transform).toBe('scale(0.94)')
    expect(button.style.boxShadow).toBe('0 0 0 5px rgba(255,255,255,0.35) inset')

    jest.advanceTimersByTime(122)

    expect(button.style.transform).toBe('scale(0.94)')
    expect(button.style.boxShadow).toBe('0 0 0 5px rgba(255,255,255,0.35) inset')

    jest.advanceTimersByTime(1)

    expect(button.style.transform).toBe('translateX(2px)')
    expect(button.style.boxShadow).toBe('rgb(1, 2, 3) 0px 0px 1px inset')
  })
})
