import { createTheme, alpha } from '@mui/material/styles'
import { themeColors } from './themeColors'
import { CSSObject } from '@mui/system'

const commonLayout = {
  'html, body, #root': {
    margin: 0,
    padding: 0,
    height: '100%',
    width: '100%',
    overflow: 'hidden',
    backgroundColor: 'inherit',
  },
  '::-webkit-scrollbar': { display: 'none' },
  '.App': { backgroundColor: 'inherit' },
  '.app-wrapper, #main, #videoContainer, .PhoneContent, .InfoContent, .CarplayContent': {
    backgroundColor: 'inherit',
  },
}

const tabRootBase = {
  position: 'sticky',
  top: 0,
  zIndex: 1200,
  width: '100%',
  boxSizing: 'border-box',
  color: 'inherit',
  cursor: 'default',
}
const tabItemBase = {
  minHeight: 64,
  color: 'inherit',
  cursor: 'default',
  '& svg': { color: 'inherit', fontSize: '36px' },
  '&.Mui-selected svg': { color: 'inherit' },
}
const buttonBaseRoot = { cursor: 'default' }
const svgIconRoot = { cursor: 'default' }

function buildTheme(mode: 'light' | 'dark') {
  const isLight = mode === 'light'
  return createTheme({
    palette: {
      mode,
      background: {
        default: isLight ? themeColors.light : themeColors.dark,
        paper: isLight ? themeColors.light : themeColors.dark,
      },
      text: {
        primary: isLight ? themeColors.textPrimaryLight : themeColors.textPrimaryDark,
        secondary: isLight ? themeColors.textSecondaryLight : themeColors.textSecondaryDark,
      },
      primary: { main: isLight ? themeColors.highlightLight : themeColors.highlightDark },
      divider: isLight ? themeColors.dividerLight : themeColors.dividerDark,
      success: { main: themeColors.successMain },
    },
    components: {
      MuiCssBaseline: {
        styleOverrides: {
          ...commonLayout,
          body: { backgroundColor: isLight ? themeColors.light : themeColors.dark },
        },
      },
      MuiTabs: {
        styleOverrides: {
          root: {
            ...(tabRootBase as CSSObject),
            backgroundColor: isLight ? themeColors.light : themeColors.dark,
          },
          indicator: {
            backgroundColor: isLight ? themeColors.highlightLight : themeColors.highlightDark,
            height: 4,
          },
        },
      },
      MuiTab: {
        styleOverrides: {
          root: tabItemBase,
        },
      },
      MuiButtonBase: {
        styleOverrides: {
          root: buttonBaseRoot,
        },
      },
      MuiSvgIcon: {
        styleOverrides: {
          root: svgIconRoot,
        },
      },
      MuiOutlinedInput: {
        styleOverrides: {
          root: {
            '&:hover .MuiOutlinedInput-notchedOutline': {
              borderColor: isLight ? themeColors.highlightLight : themeColors.highlightDark,
            },
            '&.Mui-focused .MuiOutlinedInput-notchedOutline': {
              borderColor: isLight ? themeColors.highlightLight : themeColors.highlightDark,
            },
          },
          notchedOutline: {
            borderColor: isLight ? themeColors.dividerLight : themeColors.dividerDark,
          },
        },
      },
      MuiInputLabel: {
        styleOverrides: {
          root: {
            '&.Mui-focused': {
              color: isLight ? themeColors.highlightLight : themeColors.highlightDark,
            },
          },
        },
      },
      MuiButton: {
        styleOverrides: {
          containedPrimary: {
            backgroundColor: isLight ? themeColors.highlightLight : themeColors.highlightDark,
            '&:hover': {
              backgroundColor: isLight ? themeColors.highlightAlphaLight : themeColors.highlightAlphaDark,
            },
          },
          root: {
            '&.hover-ring.MuiButton-containedPrimary:hover': {
              backgroundColor: isLight ? themeColors.highlightLight : themeColors.highlightDark,
              boxShadow: `0 0 0 2px ${
                alpha(isLight ? themeColors.highlightLight : themeColors.highlightDark, 0.55)
              } inset`,
            },
            '&.hover-ring.MuiButton-containedPrimary:focus-visible': {
              outline: 'none',
              boxShadow: `0 0 0 2px ${
                alpha(isLight ? themeColors.highlightLight : themeColors.highlightDark, 0.75)
              } inset`,
            },
          },
        },
      },
      MuiAppBar: {
        styleOverrides: {
          root: {
            backgroundColor: isLight ? themeColors.light : themeColors.dark,
            boxShadow: 'none',
          },
        },
      },
      MuiCard: {
        styleOverrides: {
          root: {
            borderRadius: 12,
            boxShadow: isLight
              ? '0 2px 8px rgba(0,0,0,0.1)'
              : '0 2px 8px rgba(0,0,0,0.3)',
          },
        },
      },
    },
  })
}

export const lightTheme = buildTheme('light')
export const darkTheme = buildTheme('dark')

export function buildRuntimeTheme(mode: 'light' | 'dark', primary?: string) {
  if (!primary) return buildTheme(mode)

  const base = buildTheme(mode)
  const isLight = mode === 'light'
  const hoverBg = isLight ? themeColors.highlightAlphaLight : themeColors.highlightAlphaDark

  return createTheme({
    ...base,
    palette: {
      ...base.palette,
      primary: { main: primary },
    },
    components: {
      ...base.components,
      MuiTabs: {
        styleOverrides: {
          ...(base.components?.MuiTabs as any)?.styleOverrides,
          indicator: {
            ...((base.components?.MuiTabs as any)?.styleOverrides?.indicator || {}),
            backgroundColor: primary,
            height: 4,
          },
        },
      },
      MuiOutlinedInput: {
        styleOverrides: {
          ...(base.components?.MuiOutlinedInput as any)?.styleOverrides,
          root: {
            ...((base.components?.MuiOutlinedInput as any)?.styleOverrides?.root || {}),
            '&:hover .MuiOutlinedInput-notchedOutline': { borderColor: primary },
            '&.Mui-focused .MuiOutlinedInput-notchedOutline': { borderColor: primary },
          },
          notchedOutline: (base.components?.MuiOutlinedInput as any)?.styleOverrides?.notchedOutline,
        },
      },
      MuiInputLabel: {
        styleOverrides: {
          ...(base.components?.MuiInputLabel as any)?.styleOverrides,
          root: {
            ...((base.components?.MuiInputLabel as any)?.styleOverrides?.root || {}),
            '&.Mui-focused': { color: primary },
          },
        },
      },
      MuiButton: {
        styleOverrides: {
          ...(base.components?.MuiButton as any)?.styleOverrides,
          containedPrimary: {
            ...((base.components?.MuiButton as any)?.styleOverrides?.containedPrimary || {}),
            backgroundColor: primary,
            '&:hover': { backgroundColor: hoverBg },
          },
          root: {
            ...((base.components?.MuiButton as any)?.styleOverrides?.root || {}),
            '&.hover-ring.MuiButton-containedPrimary:hover': {
              backgroundColor: primary,
              boxShadow: `0 0 0 2px ${alpha(primary, 0.55)} inset`,
            },
            '&.hover-ring.MuiButton-containedPrimary:focus-visible': {
              outline: 'none',
              boxShadow: `0 0 0 2px ${alpha(primary, 0.75)} inset`,
            },
          },
        },
      },
    },
  })
}

export function initCursorHider(inactivityMs: number = 5000) {
  let timer: ReturnType<typeof setTimeout>
  const setCursor = (value: string) => {
    const elems = [
      document.body,
      document.getElementById('main'),
      ...Array.from(
        document.querySelectorAll<HTMLElement>(
          '.MuiTabs-root, .MuiTab-root, .MuiButtonBase-root, .MuiSvgIcon-root'
        )
      ),
    ].filter((el): el is HTMLElement => el !== null)
    elems.forEach(el => el.style.setProperty('cursor', value, 'important'))
  }
  function reset() {
    clearTimeout(timer)
    setCursor('default')
    timer = setTimeout(() => setCursor('none'), inactivityMs)
  }
  document.addEventListener('mousemove', reset)
  reset()
}
