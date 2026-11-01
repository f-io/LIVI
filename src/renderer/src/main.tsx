import { CssBaseline, ThemeProvider } from '@mui/material'
import { useCallback, useMemo, useState } from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import AuxApp from './AuxApp'
import DashApp from './DashApp'
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
import { themeColors } from './themeColors'
import './i18n'
import { getWindowRole } from './utils/windowRole'

const role = getWindowRole()
if (role === 'main') {
  initUiBreatheClock()
  initCursorHider()
}
if (role === 'dash') document.title = 'Dash'
else if (role === 'aux') document.title = 'Auxiliary'

// Under the livi-compositor the window is transparent so the GPU video plane behind it shows through
if (window.app?.compositor) document.documentElement.classList.add('compositor')

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
    typeof settings?.darkMode === 'boolean'
      ? settings.darkMode
        ? THEME.DARK
        : THEME.LIGHT
      : THEME.DARK

  // Night (low glare): muted red accent replaces amber / custom accent colours
  const isNight = settings?.nightMode === true

  const primaryOverride = isNight
    ? themeColors.nightAccent
    : mode === THEME.DARK
      ? settings?.primaryColorDark
      : settings?.primaryColorLight

  const highlightOverride = isNight
    ? themeColors.nightHighlight
    : mode === THEME.DARK
      ? settings?.highlightColorDark
      : settings?.highlightColorLight

  const backgroundOverride =
    mode === THEME.DARK ? settings?.backgroundColorDark : settings?.backgroundColorLight

  const theme = useMemo(() => {
    return primaryOverride || highlightOverride || backgroundOverride
      ? buildRuntimeTheme(mode, primaryOverride, highlightOverride, backgroundOverride)
      : mode === THEME.DARK
        ? darkTheme
        : lightTheme
  }, [mode, primaryOverride, highlightOverride, backgroundOverride])

  const providerValue = useMemo(
    () => ({
      ...appContext,
      onSetAppContext: handleChangeAppContext
    }),
    [appContext, handleChangeAppContext]
  )

  const RoleApp = role === 'dash' ? DashApp : role === 'aux' ? AuxApp : App

  return (
    <AppContext.Provider value={providerValue}>
      <ThemeProvider theme={theme}>
        <CssBaseline enableColorScheme />
        <RoleApp />
      </ThemeProvider>
    </AppContext.Provider>
  )
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(<Root />)
