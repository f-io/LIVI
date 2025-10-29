import ReactDOM from 'react-dom/client'
import App from './App'
import { ThemeProvider, CssBaseline } from '@mui/material'
import { useCarplayStore } from './store/store'
import { darkTheme, lightTheme, buildRuntimeTheme, initCursorHider } from './theme'
import '@fontsource/roboto/300.css'
import '@fontsource/roboto/400.css'
import '@fontsource/roboto/500.css'
import '@fontsource/roboto/700.css'
import { useMemo } from 'react'

initCursorHider();

const Root = () => {
  const settings = useCarplayStore(state => state.settings)

  const isDark = settings ? !!settings.nightMode : true

  const override = useMemo(() => {
    const raw = isDark ? settings?.primaryColorDark : settings?.primaryColorLight
    if (typeof raw !== 'string') return undefined
    const c = raw.trim()
    return /^#([0-9a-f]{6}|[0-9a-f]{3})$/i.test(c) ? c : undefined
  }, [settings, isDark])

  const theme = useMemo(() => {
    return override
      ? buildRuntimeTheme(isDark ? 'dark' : 'light', override)
      : (isDark ? darkTheme : lightTheme)
  }, [isDark, override])

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline enableColorScheme />
      <App />
    </ThemeProvider>
  )
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(<Root />)
