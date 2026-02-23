import ReactDOM from 'react-dom/client'
import App from './App'
import { ThemeProvider, CssBaseline } from '@mui/material'
import { useCarplayStore } from './store/store'
import {
  darkTheme,
  lightTheme,
  buildRuntimeTheme,
  initCursorHider,
  initUiBreatheClock
} from './theme'
import { useCallback, useMemo, useState } from 'react'
import '@fontsource/roboto/300.css'
import '@fontsource/roboto/400.css'
import '@fontsource/roboto/500.css'
import '@fontsource/roboto/700.css'
import { AppContext, type AppContextProps } from './context'
import { THEME } from './constants'
import './i18n'

initUiBreatheClock()
initCursorHider()

const Root = () => {
  const settings = useCarplayStore((s) => s.settings)

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
