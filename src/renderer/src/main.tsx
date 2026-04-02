import { CssBaseline, ThemeProvider } from '@mui/material'
import { useCallback, useMemo, useState } from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { useLiviStore } from './store/store'
import {
  buildRuntimeTheme,
  darkTheme,
  initCursorHider,
  initUiBreatheClock,
  lightTheme
} from './theme'
import '@fontsource/roboto/300.css'
import '@fontsource/roboto/400.css'
import '@fontsource/roboto/500.css'
import '@fontsource/roboto/700.css'
import { THEME } from './constants'
import { AppContext, type AppContextProps } from './context'
import './i18n'

initUiBreatheClock()
initCursorHider()

export const Root = () => {
  const settings = useLiviStore((s) => s.settings)

  // detect touch, stylus and mouse
  const isTouchDevice =
    navigator.maxTouchPoints >= 0 || window.matchMedia('(pointer: coarse)').matches

  const [appContext, setAppContext] = useState<AppContextProps>({
    isTouchDevice
  })

  const handleChangeAppContext = useCallback((patch: Partial<AppContextProps>) => {
    setAppContext((prev) => ({
      ...prev,
      ...patch
    }))
  }, [])

  const mode: THEME.DARK | THEME.LIGHT =
    typeof settings?.nightMode === 'boolean'
      ? settings.nightMode
        ? THEME.DARK
        : THEME.LIGHT
      : THEME.DARK

  const primaryOverride =
    mode === THEME.DARK ? settings?.primaryColorDark : settings?.primaryColorLight

  const highlightOverride =
    mode === THEME.DARK ? settings?.highlightColorDark : settings?.highlightColorLight

  const theme = useMemo(() => {
    return primaryOverride || highlightOverride
      ? buildRuntimeTheme(mode, primaryOverride, highlightOverride)
      : mode === THEME.DARK
        ? darkTheme
        : lightTheme
  }, [mode, primaryOverride, highlightOverride])

  const providerValue = useMemo(
    () => ({
      ...appContext,
      onSetAppContext: handleChangeAppContext
    }),
    [appContext, handleChangeAppContext]
  )

  return (
    <AppContext.Provider value={providerValue}>
      <ThemeProvider theme={theme}>
        <CssBaseline enableColorScheme />
        <App />
      </ThemeProvider>
    </AppContext.Provider>
  )
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(<Root />)
