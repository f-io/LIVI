import { renderHook } from '@testing-library/react'
import { useActiveControl } from '../useActivateControl'

describe('useActiveControl', () => {
  test('returns false for null element', () => {
    const { result } = renderHook(() => useActiveControl())
    expect(result.current(null)).toBe(false)
  })

  test('clicks switch-like element', () => {
    const { result } = renderHook(() => useActiveControl())
    const input = document.createElement('input')
    input.type = 'checkbox'
    const click = jest.spyOn(input, 'click')
    expect(result.current(input)).toBe(true)
    expect(click).toHaveBeenCalled()
  })

  test('dispatches mousedown for dropdown button', () => {
    const { result } = renderHook(() => useActiveControl())
    const el = document.createElement('div')
    el.setAttribute('role', 'combobox')
    el.setAttribute('aria-haspopup', 'listbox')
    const dispatch = jest.spyOn(el, 'dispatchEvent')
    expect(result.current(el)).toBe(true)
    expect(dispatch).toHaveBeenCalled()
  })
})
