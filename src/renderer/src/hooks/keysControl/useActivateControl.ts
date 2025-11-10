import { useCallback } from 'react'

export const useActiveControl = () => {
  return useCallback((el: HTMLElement | null) => {
    if (!el) return false

    const isSwitchLike =
      el.getAttribute('role') === 'switch' ||
      (el.tagName === 'INPUT' && (el as HTMLInputElement).type === 'checkbox')

    const isDropdownButton =
      el.getAttribute('role') === 'combobox' && el.getAttribute('aria-haspopup') === 'listbox'

    const clickable =
      el.closest<HTMLElement>(
        '[role="button"][aria-haspopup="listbox"],[role="switch"],button,[role="button"],a,label,[for]'
      ) ||
      el.querySelector<HTMLElement>(
        '[role="button"][aria-haspopup="listbox"],[role="switch"],button,[role="button"],a,label,[for]'
      ) ||
      el.querySelector<HTMLElement>(
        '[role="combobox"][aria-haspopup="listbox"],[role="switch"],button,[role="button"],a,label,[for]'
      ) ||
      el

    if (isSwitchLike || (typeof clickable.click === 'function' && !isDropdownButton)) {
      clickable.click()
      return true
    }

    if (isDropdownButton) {
      const evt = new MouseEvent('mousedown', { bubbles: true, cancelable: true })
      clickable.dispatchEvent(evt)
      return true
    }

    const evt = new MouseEvent('click', { bubbles: true, cancelable: true })
    return clickable.dispatchEvent(evt)
  }, [])
}
