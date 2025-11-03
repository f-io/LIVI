import ReactDOM from 'react-dom/client'
import App from './App'
import { ThemeProvider, CssBaseline } from '@mui/material'
import { useCarplayStore } from './store/store'
import { darkTheme, lightTheme, buildRuntimeTheme, initCursorHider } from './theme'
import { useMemo } from 'react'
import '@fontsource/roboto/300.css'
import '@fontsource/roboto/400.css'
import '@fontsource/roboto/500.css'
import '@fontsource/roboto/700.css'

initCursorHider()

const Root = () => {
  const settings = useCarplayStore((s) => s.settings)

  const mode: 'dark' | 'light' =
    typeof settings?.nightMode === 'boolean' ? (settings.nightMode ? 'dark' : 'light') : 'dark'

  const override = mode === 'dark' ? settings?.primaryColorDark : settings?.primaryColorLight

  const theme = useMemo(() => {
    return override ? buildRuntimeTheme(mode, override) : mode === 'dark' ? darkTheme : lightTheme
  }, [mode, override])

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline enableColorScheme />
      <App />
    </ThemeProvider>
  )
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(<Root />)
