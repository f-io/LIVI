import { act, renderHook } from '@testing-library/react'
import { useNavbarHidden } from '../useNavbarHidden'

describe('useNavbarHidden', () => {
  test('reads initial hidden state and exposes setter', () => {
    const el = document.createElement('div')
    el.id = 'content-root'
    el.setAttribute('data-nav-hidden', '1')
    document.body.appendChild(el)

    const observe = jest.fn()
    const disconnect = jest.fn()
    ;(global as any).MutationObserver = jest.fn(() => ({ observe, disconnect }))

    const { result, unmount } = renderHook(() => useNavbarHidden())
    expect(result.current.isNavbarHidden).toBe(true)

    act(() => {
      result.current.onSetNavHidden(false)
    })
    expect(result.current.isNavbarHidden).toBe(false)

    unmount()
    expect(disconnect).toHaveBeenCalled()
    el.remove()
  })
})
