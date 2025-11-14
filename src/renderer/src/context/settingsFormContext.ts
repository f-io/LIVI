import { createContext, RefObject } from 'react'

export type AppContextProps = {
  navEl?: RefObject<HTMLElement | null> | null
  contentEl?: RefObject<HTMLElement | null> | null
  isTouchDevice: boolean
  keyboardNavigation?: {
    focusedElId?: string | null
  }
  onSetAppContext?: (appContext: AppContextProps) => void
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
