import ReactDOM from 'react-dom/client'
import App from './App'
import { ThemeProvider, CssBaseline } from '@mui/material'
import { useCarplayStore } from './store/store'
import { darkTheme, lightTheme, buildRuntimeTheme, initCursorHider } from './theme'
import { useCallback, useMemo, useState } from 'react'
import '@fontsource/roboto/300.css'
import '@fontsource/roboto/400.css'
import '@fontsource/roboto/500.css'
import '@fontsource/roboto/700.css'
import { AppContext, AppContextProps } from './context'
import { THEME } from './constants'

initCursorHider()

const Root = () => {
  const settings = useCarplayStore((s) => s.settings)
  const [appContext, setAppContext] = useState<AppContextProps>()

  const handleChangeAppContext = useCallback(
    (props: AppContextProps) => {
      setAppContext({
        ...appContext,
        ...props
      })
    },
    [appContext]
  )

  const mode: THEME.DARK | THEME.LIGHT =
    typeof settings?.nightMode === 'boolean'
      ? settings.nightMode
        ? THEME.DARK
        : THEME.LIGHT
      : THEME.DARK

  const override = mode === THEME.DARK ? settings?.primaryColorDark : settings?.primaryColorLight

  const theme = useMemo(() => {
    return override
      ? buildRuntimeTheme(mode, override)
      : mode === THEME.DARK
        ? darkTheme
        : lightTheme
  }, [mode, override])

  return (
    <AppContext.Provider value={{ ...appContext, onSetAppContext: handleChangeAppContext }}>
      <ThemeProvider theme={theme}>
        <CssBaseline enableColorScheme />
        <App />
      </ThemeProvider>
    </AppContext.Provider>
  )
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(<Root />)
