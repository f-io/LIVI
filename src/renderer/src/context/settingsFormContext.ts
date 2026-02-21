import { createContext, RefObject } from 'react'

export type AppContextProps = {
  navEl?: RefObject<HTMLElement | null> | null
  contentEl?: RefObject<HTMLElement | null> | null
  isTouchDevice: boolean
  keyboardNavigation?: {
    focusedElId?: string | null
  }

  telemetryPager?: {
    prev: () => void
    next: () => void
    canPrev: () => boolean
    canNext: () => boolean
  }

  onSetAppContext?: (patch: Partial<AppContextProps>) => void
}

export const AppContext = createContext<AppContextProps>({
  navEl: null,
  contentEl: null,
  isTouchDevice: false,
  keyboardNavigation: {
    focusedElId: null
  },
  onSetAppContext: undefined
})
