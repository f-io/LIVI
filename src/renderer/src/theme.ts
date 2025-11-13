import { createTheme, alpha } from '@mui/material/styles'
import { themeColors } from './themeColors'
import { CSSObject } from '@mui/system'
import { THEME } from './constants'

const commonLayout = {
  'html, body, #root': {
    margin: 0,
    padding: 0,
    height: '100%',
    width: '100%',
    overflow: 'hidden',
    backgroundColor: 'inherit'
  },
  '::-webkit-scrollbar': { display: 'none' },
  '.App': { backgroundColor: 'inherit' },
  '.app-wrapper, #main, #videoContainer, .PhoneContent, .InfoContent, .CarplayContent': {
    backgroundColor: 'inherit'
  }
}

const tabRootBase = {
  position: 'sticky',
  top: 0,
  zIndex: 1200,
  width: '100%',
  boxSizing: 'border-box',
  color: 'inherit',
  cursor: 'default'
}
const tabItemBase = {
  minHeight: 64,
  color: 'inherit',
  cursor: 'default',
  '& svg': { color: 'inherit', fontSize: '36px' },
  '&.Mui-selected svg': { color: 'inherit' }
}
const buttonBaseRoot = { cursor: 'default' }
const svgIconRoot = { cursor: 'default' }

function buildTheme(mode: THEME.LIGHT | THEME.DARK) {
  const isLight = mode === THEME.LIGHT
  return createTheme({
    breakpoints: {
      values: {
        xs: 0, // default value
        sm: 760, // customized value from 600
        md: 900, // default value
        lg: 1200, // default value
        xl: 1536 // default value
      }
    },
    palette: {
      mode,
      background: {
        default: isLight ? themeColors.light : themeColors.dark,
        paper: isLight ? themeColors.light : themeColors.dark
      },
      text: {
        primary: isLight ? themeColors.textPrimaryLight : themeColors.textPrimaryDark,
        secondary: isLight ? themeColors.textSecondaryLight : themeColors.textSecondaryDark
      },
      primary: { main: isLight ? themeColors.highlightLight : themeColors.highlightDark },
      divider: isLight ? themeColors.dividerLight : themeColors.dividerDark,
      success: { main: themeColors.successMain }
    },
    components: {
      MuiCssBaseline: {
        styleOverrides: {
          ...commonLayout,
          body: { backgroundColor: isLight ? themeColors.light : themeColors.dark }
        }
      },
      MuiTabs: {
        styleOverrides: {
          root: {
            ...(tabRootBase as CSSObject),
            backgroundColor: isLight ? themeColors.light : themeColors.dark
          },
          indicator: {
            backgroundColor: isLight ? themeColors.highlightLight : themeColors.highlightDark,
            height: 4
          }
        }
      },
      MuiTab: {
        styleOverrides: {
          root: tabItemBase
        }
      },
      MuiButtonBase: {
        styleOverrides: {
          root: buttonBaseRoot
        }
      },
      MuiSvgIcon: {
        styleOverrides: {
          root: svgIconRoot
        }
      },
      MuiOutlinedInput: {
        styleOverrides: {
          root: {
            '&:hover .MuiOutlinedInput-notchedOutline': {
              borderColor: isLight
                ? themeColors.highlightFocusedFieldLight
                : themeColors.highlightFocusedFieldDark
            },
            '&.Mui-focused .MuiOutlinedInput-notchedOutline': {
              borderColor: isLight
                ? themeColors.highlightFocusedFieldLight
                : themeColors.highlightFocusedFieldDark,
              borderWidth: '2px'
            }
          },
          notchedOutline: {
            borderColor: isLight ? themeColors.dividerLight : themeColors.dividerDark
          }
        }
      },
      MuiInputLabel: {
        styleOverrides: {
          root: {
            '&.Mui-focused': {
              color: isLight ? themeColors.highlightLight : themeColors.highlightDark
            }
          }
        }
      },
      MuiButton: {
        styleOverrides: {
          containedPrimary: {
            backgroundColor: isLight ? themeColors.highlightLight : themeColors.highlightDark,
            '&:hover': {
              backgroundColor: isLight
                ? themeColors.highlightAlphaLight
                : themeColors.highlightAlphaDark
            }
          },
          root: {
            '&.hover-ring.MuiButton-containedPrimary:hover': {
              backgroundColor: isLight ? themeColors.highlightLight : themeColors.highlightDark,
              boxShadow: `0 0 0 2px ${alpha(isLight ? themeColors.highlightLight : themeColors.highlightDark, 0.55)} inset`
            },
            '&.hover-ring.MuiButton-containedPrimary:focus-visible': {
              outline: 'none',
              boxShadow: `0 0 0 2px ${alpha(isLight ? themeColors.highlightLight : themeColors.highlightDark, 0.75)} inset`
            }
          }
        }
      },
      MuiAppBar: {
        styleOverrides: {
          root: {
            backgroundColor: isLight ? themeColors.light : themeColors.dark,
            boxShadow: 'none'
          }
        }
      },
      MuiCard: {
        styleOverrides: {
          root: {
            borderRadius: 12,
            boxShadow: isLight ? '0 2px 8px rgba(0,0,0,0.1)' : '0 2px 8px rgba(0,0,0,0.3)'
          }
        }
      }
    }
  })
}

export const lightTheme = buildTheme(THEME.LIGHT)
export const darkTheme = buildTheme(THEME.DARK)

export function buildRuntimeTheme(mode: THEME.LIGHT | THEME.DARK, primary?: string) {
  if (!primary) return buildTheme(mode)

  const base = buildTheme(mode)
  const isLight = mode === THEME.LIGHT
  const hoverBg = isLight ? themeColors.highlightAlphaLight : themeColors.highlightAlphaDark

  const tabsSO = (base.components?.MuiTabs?.styleOverrides ?? {}) as Record<string, CSSObject>
  const outlinedSO = (base.components?.MuiOutlinedInput?.styleOverrides ?? {}) as Record<
    string,
    CSSObject
  >
  const inputLabelSO = (base.components?.MuiInputLabel?.styleOverrides ?? {}) as Record<
    string,
    CSSObject
  >
  const buttonSO = (base.components?.MuiButton?.styleOverrides ?? {}) as Record<string, CSSObject>

  const tabsIndicator = (tabsSO.indicator ?? {}) as CSSObject
  const outlinedRoot = (outlinedSO.root ?? {}) as CSSObject
  const outlinedNotched = (outlinedSO.notchedOutline ?? {}) as CSSObject
  const inputLabelRoot = (inputLabelSO.root ?? {}) as CSSObject
  const btnContainedPrimary = (buttonSO.containedPrimary ?? {}) as CSSObject
  const btnRoot = (buttonSO.root ?? {}) as CSSObject

  return createTheme({
    ...base,
    palette: {
      ...base.palette,
      primary: { main: primary }
    },
    components: {
      ...base.components,

      MuiTabs: {
        styleOverrides: {
          ...tabsSO,
          indicator: {
            ...tabsIndicator,
            backgroundColor: primary,
            height: 4
          }
        }
      },

      MuiOutlinedInput: {
        styleOverrides: {
          ...outlinedSO,
          root: {
            ...outlinedRoot,
            '&:hover .MuiOutlinedInput-notchedOutline': { borderColor: 'white' },
            '&.Mui-focused .MuiOutlinedInput-notchedOutline': { borderColor: 'white' }
          },
          notchedOutline: outlinedNotched
        }
      },

      MuiInputLabel: {
        styleOverrides: {
          ...inputLabelSO,
          root: {
            ...inputLabelRoot,
            '&.Mui-focused': { color: primary }
          }
        }
      },

      MuiButton: {
        styleOverrides: {
          ...buttonSO,
          containedPrimary: {
            ...btnContainedPrimary,
            backgroundColor: primary,
            '&:hover': { backgroundColor: hoverBg }
          },
          root: {
            ...btnRoot,
            '&.hover-ring.MuiButton-containedPrimary:hover': {
              backgroundColor: primary,
              boxShadow: `0 0 0 2px ${alpha(primary, 0.55)} inset`
            },
            '&.hover-ring.MuiButton-containedPrimary:focus-visible': {
              outline: 'none',
              boxShadow: `0 0 0 2px ${alpha(primary, 0.75)} inset`
            }
          }
        }
      }
    }
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
      )
    ].filter((el): el is HTMLElement => el !== null)
    elems.forEach((el) => el.style.setProperty('cursor', value, 'important'))
  }
  function reset() {
    clearTimeout(timer)
    setCursor('default')
    timer = setTimeout(() => setCursor('none'), inactivityMs)
  }
  document.addEventListener('mousemove', reset)
  reset()
}
