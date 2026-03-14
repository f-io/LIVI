import { renderHook } from '@testing-library/react'
import { ReactNode } from 'react'
import { AppContext, AppContextProps } from '../../../context'
import { useFocus } from '../useFocus'

const wrapperWithContext = (context: AppContextProps) => {
  return ({ children }: { children: ReactNode }) => (
    <AppContext.Provider value={context}>{children}</AppContext.Provider>
  )
}

describe('useFocus', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
  })

  test('detects visible and form elements', () => {
    const { result } = renderHook(() => useFocus(), {
      wrapper: wrapperWithContext({ isTouchDevice: false })
    })

    const hidden = document.createElement('button')
    hidden.setAttribute('hidden', 'true')
    const disabled = document.createElement('input')
    disabled.setAttribute('disabled', 'true')
    const visible = document.createElement('div')

    expect(result.current.isVisible(hidden)).toBe(false)
    expect(result.current.isVisible(disabled)).toBe(false)
    expect(result.current.isVisible(visible)).toBe(true)

    const input = document.createElement('input')
    const select = document.createElement('select')
    const textarea = document.createElement('textarea')
    const slider = document.createElement('div')
    slider.setAttribute('role', 'slider')
    const editable = document.createElement('div')
    editable.setAttribute('contenteditable', 'true')

    expect(result.current.isFormField(input)).toBe(true)
    expect(result.current.isFormField(select)).toBe(true)
    expect(result.current.isFormField(textarea)).toBe(true)
    expect(result.current.isFormField(slider)).toBe(true)
    expect(result.current.isFormField(editable)).toBe(true)
    expect(result.current.isFormField(null)).toBe(false)
  })

  test('returns focusable list and first focusable using seed and non-form fallback', () => {
    const root = document.createElement('div')

    const formInput = document.createElement('input')
    const hiddenBtn = document.createElement('button')
    hiddenBtn.setAttribute('hidden', 'true')
    const seeded = document.createElement('button')
    seeded.dataset.seed = 'first'
    const link = document.createElement('a')
    link.href = '#x'

    root.appendChild(formInput)
    root.appendChild(hiddenBtn)
    root.appendChild(seeded)
    root.appendChild(link)
    document.body.appendChild(root)

    const { result } = renderHook(() => useFocus(), {
      wrapper: wrapperWithContext({ isTouchDevice: false })
    })

    const list = result.current.getFocusableList(root)
    expect(list).toContain(formInput)
    expect(list).not.toContain(hiddenBtn)
    expect(list).toContain(seeded)

    expect(result.current.getFirstFocusable(root)).toBe(seeded)

    seeded.remove()
    expect(result.current.getFirstFocusable(root)).toBe(link)
  })

  test('focusSelectedNav and focusFirstInMain work with context refs', () => {
    const navRoot = document.createElement('div')
    navRoot.id = 'nav-root'
    const selectedTab = document.createElement('button')
    selectedTab.setAttribute('role', 'tab')
    selectedTab.setAttribute('aria-selected', 'true')
    navRoot.appendChild(selectedTab)
    document.body.appendChild(navRoot)

    const contentRoot = document.createElement('div')
    contentRoot.id = 'content-root'
    const mainBtn = document.createElement('button')
    contentRoot.appendChild(mainBtn)
    document.body.appendChild(contentRoot)

    const context: AppContextProps = {
      isTouchDevice: false,
      navEl: { current: navRoot },
      contentEl: { current: contentRoot },
      keyboardNavigation: { focusedElId: null },
      onSetAppContext: jest.fn()
    }

    const { result } = renderHook(() => useFocus(), {
      wrapper: wrapperWithContext(context)
    })

    expect(result.current.focusSelectedNav()).toBe(true)
    expect(document.activeElement).toBe(selectedTab)

    expect(result.current.focusFirstInMain()).toBe(true)
    expect(document.activeElement).toBe(mainBtn)
  })

  test('prefers active dialog as main root', () => {
    const contentRoot = document.createElement('div')
    contentRoot.id = 'content-root'
    const contentBtn = document.createElement('button')
    contentRoot.appendChild(contentBtn)
    document.body.appendChild(contentRoot)

    const dialog = document.createElement('div')
    dialog.setAttribute('role', 'dialog')
    const dialogBtn = document.createElement('button')
    dialog.appendChild(dialogBtn)
    document.body.appendChild(dialog)

    const context: AppContextProps = {
      isTouchDevice: false,
      contentEl: { current: contentRoot },
      keyboardNavigation: { focusedElId: null },
      onSetAppContext: jest.fn()
    }

    const { result } = renderHook(() => useFocus(), {
      wrapper: wrapperWithContext(context)
    })

    expect(result.current.focusFirstInMain()).toBe(true)
    expect(document.activeElement).toBe(dialogBtn)

    dialog.setAttribute('aria-hidden', 'true')
    expect(result.current.focusFirstInMain()).toBe(true)
    expect(document.activeElement).toBe(contentBtn)
  })

  test('moves focus linearly and resets keyboard focused id', () => {
    const onSetAppContext = jest.fn()
    const contentRoot = document.createElement('div')
    contentRoot.id = 'content-root'

    const scrolledWrapper = document.createElement('div')
    scrolledWrapper.setAttribute('data-scrolled-wrapper', 'true')
    contentRoot.appendChild(scrolledWrapper)

    const first = document.createElement('button')
    const second = document.createElement('button')
    scrolledWrapper.appendChild(first)
    scrolledWrapper.appendChild(second)

    document.body.appendChild(contentRoot)

    Object.defineProperty(second, 'getBoundingClientRect', {
      value: () => ({
        top: 100,
        bottom: 150,
        left: 0,
        right: 0,
        width: 10,
        height: 50,
        x: 0,
        y: 0,
        toJSON: () => ({})
      })
    })

    Object.defineProperty(scrolledWrapper, 'getBoundingClientRect', {
      value: () => ({
        top: 0,
        bottom: 80,
        left: 0,
        right: 0,
        width: 100,
        height: 80,
        x: 0,
        y: 0,
        toJSON: () => ({})
      })
    })

    const context: AppContextProps = {
      isTouchDevice: false,
      contentEl: { current: contentRoot },
      keyboardNavigation: { focusedElId: 'x' },
      onSetAppContext
    }

    const { result } = renderHook(() => useFocus(), {
      wrapper: wrapperWithContext(context)
    })

    first.focus()
    expect(result.current.moveFocusLinear(1)).toBe(true)
    expect(document.activeElement).toBe(second)
    expect(onSetAppContext).toHaveBeenCalled()

    expect(result.current.moveFocusLinear(-1)).toBe(true)
    expect(document.activeElement).toBe(first)
  })
})
