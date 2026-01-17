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
  const primary = isLight ? themeColors.primaryColorLight : themeColors.primaryColorDark
  const highlight = isLight ? themeColors.highlightColorLight : themeColors.highlightColorDark

  return createTheme({
    breakpoints: {
      values: {
        xs: 0,
        sm: 760,
        md: 900,
        lg: 1200,
        xl: 1536
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
      primary: { main: primary },
      divider: isLight ? themeColors.dividerLight : themeColors.dividerDark,
      success: { main: themeColors.successMain }
    },
    components: {
      MuiCssBaseline: {
        styleOverrides: {
          ...commonLayout,
          body: {
            backgroundColor: isLight ? themeColors.light : themeColors.dark,
            '--ui-highlight': highlight
          },
          '.fft-surface': {
            backgroundColor: isLight ? themeColors.fftSurfaceLight : themeColors.fftSurfaceDark,
            ...(isLight
              ? {}
              : {
                  backgroundImage:
                    'radial-gradient(ellipse at center, rgba(255,255,255,0.05) 0%, rgba(0,0,0,0) 60%)'
                })
          },

          '.fft-surface-inner': {
            width: '100%',
            height: '100%',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center'
          },

          '.artwork-surface': {
            backgroundColor: isLight
              ? themeColors.artworkSurfaceLight
              : themeColors.artworkSurfaceDark
          },

          ':focus': {
            outline: 'none'
          }
        }
      },

      MuiTabs: {
        styleOverrides: {
          root: {
            ...(tabRootBase as CSSObject),
            backgroundColor: isLight ? themeColors.light : themeColors.dark
          },
          indicator: {
            backgroundColor: highlight,
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
          root: {
            ...buttonBaseRoot,
            '& .MuiTouchRipple-child': {
              backgroundColor: `${alpha(highlight, 0.75)} !important`
            }
          }
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

      MuiIconButton: {
        defaultProps: {
          disableRipple: false,
          disableFocusRipple: false,
          disableTouchRipple: false
        },
        styleOverrides: {
          root: {
            backgroundColor: 'transparent',
            boxShadow: 'none',
            outline: 'none',

            '&:hover': { backgroundColor: 'transparent' },
            '&.Mui-focusVisible': { backgroundColor: 'transparent' },
            '&.nav-focus-primary .MuiSvgIcon-root': {
              transition: 'color 120ms ease-out'
            },
            '&.nav-focus-primary:hover .MuiSvgIcon-root, &.nav-focus-primary.Mui-focusVisible .MuiSvgIcon-root':
              {
                color: primary
              }
          }
        }
      },

      MuiSwitch: {
        defaultProps: {
          disableRipple: false,
          disableFocusRipple: false,
          disableTouchRipple: false
        },
        styleOverrides: {
          switchBase: {
            '&.Mui-checked': {
              color: `${primary} !important`
            },

            '&.Mui-checked + .MuiSwitch-track': {
              backgroundColor: `${primary} !important`,
              opacity: 1
            },
            '&.Mui-focusVisible': {
              backgroundColor: 'transparent'
            }
          },
          thumb: { boxShadow: 'none' },
          track: { opacity: 1 }
        }
      },

      MuiSlider: {
        styleOverrides: {
          thumb: {
            '&.Mui-focusVisible, &.Mui-active, &:hover': {
              boxShadow: `0 0 0 10px ${alpha(highlight, 0.75)}`
            }
          }
        }
      },

      MuiInputLabel: {
        styleOverrides: {
          root: {
            '&.Mui-focused': {
              color: highlight
            }
          }
        }
      },

      MuiButton: {
        styleOverrides: {
          containedPrimary: {
            backgroundColor: primary,
            '&:hover': {
              backgroundColor: primary,
              boxShadow: `0 0 0 2px ${alpha(highlight, 0.55)} inset, 0 0 14px ${alpha(highlight, 0.45)}`
            },
            '&:active': {
              backgroundColor: primary,
              boxShadow: `0 0 0 2px ${alpha(highlight, 0.65)} inset, 0 0 18px ${alpha(highlight, 0.5)}`
            }
          },
          root: {
            '&.MuiButton-containedPrimary:focus-visible': {
              outline: 'none',
              boxShadow: `0 0 0 2px ${alpha(highlight, 0.75)} inset, 0 0 18px ${alpha(highlight, 0.65)}`
            },

            '&.hover-ring.MuiButton-containedPrimary:hover': {
              backgroundColor: primary,
              boxShadow: `0 0 0 2px ${alpha(highlight, 0.65)} inset, 0 0 16px ${alpha(highlight, 0.55)}`
            },
            '&.hover-ring.MuiButton-containedPrimary:focus-visible': {
              outline: 'none',
              boxShadow: `0 0 0 2px ${alpha(highlight, 0.85)} inset, 0 0 20px ${alpha(highlight, 0.7)}`
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

export function buildRuntimeTheme(
  mode: THEME.LIGHT | THEME.DARK,
  primary?: string,
  highlight?: string
) {
  if (!primary && !highlight) return buildTheme(mode)

  const base = buildTheme(mode)

  if (!primary) {
    primary = mode === THEME.LIGHT ? themeColors.primaryColorLight : themeColors.primaryColorDark
  }
  if (!highlight) {
    highlight =
      mode === THEME.LIGHT ? themeColors.highlightColorLight : themeColors.highlightColorDark
  }

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
  const buttonBaseSO = (base.components?.MuiButtonBase?.styleOverrides ?? {}) as Record<
    string,
    CSSObject
  >
  const iconButtonSO = (base.components?.MuiIconButton?.styleOverrides ?? {}) as Record<
    string,
    CSSObject
  >
  const switchSO = (base.components?.MuiSwitch?.styleOverrides ?? {}) as Record<string, CSSObject>
  const cssBaselineSO = (base.components?.MuiCssBaseline?.styleOverrides ?? {}) as Record<
    string,
    any
  >

  const tabsIndicator = (tabsSO.indicator ?? {}) as CSSObject
  const outlinedRoot = (outlinedSO.root ?? {}) as CSSObject
  const outlinedNotched = (outlinedSO.notchedOutline ?? {}) as CSSObject
  const inputLabelRoot = (inputLabelSO.root ?? {}) as CSSObject
  const btnContainedPrimary = (buttonSO.containedPrimary ?? {}) as CSSObject
  const btnRoot = (buttonSO.root ?? {}) as CSSObject
  const btnBaseRoot = (buttonBaseSO.root ?? {}) as CSSObject

  const iconBtnRoot = (iconButtonSO.root ?? {}) as CSSObject
  const swSwitchBase = (switchSO.switchBase ?? {}) as CSSObject
  const swThumb = (switchSO.thumb ?? {}) as CSSObject
  const swTrack = (switchSO.track ?? {}) as CSSObject

  return createTheme({
    ...base,
    palette: {
      ...base.palette,
      primary: { main: primary! }
    },

    components: {
      ...base.components,

      MuiCssBaseline: {
        styleOverrides: {
          ...cssBaselineSO,
          body: {
            ...(cssBaselineSO.body ?? {}),
            '--ui-highlight': highlight!
          },
          ':focus': { outline: 'none' }
        }
      },

      MuiTabs: {
        styleOverrides: {
          ...tabsSO,
          indicator: {
            ...tabsIndicator,
            backgroundColor: highlight!,
            height: 4
          }
        }
      },

      MuiOutlinedInput: {
        styleOverrides: {
          ...outlinedSO,
          root: outlinedRoot,
          notchedOutline: outlinedNotched
        }
      },

      MuiInputLabel: {
        styleOverrides: {
          ...inputLabelSO,
          root: {
            ...inputLabelRoot,
            '&.Mui-focused': { color: highlight! }
          }
        }
      },

      MuiButtonBase: {
        styleOverrides: {
          ...buttonBaseSO,
          root: {
            ...btnBaseRoot,
            cursor: 'default',

            '& .MuiTouchRipple-child': {
              backgroundColor: `${alpha(highlight!, 0.75)} !important`
            }
          }
        }
      },

      MuiButton: {
        styleOverrides: {
          ...buttonSO,
          containedPrimary: {
            ...btnContainedPrimary,
            backgroundColor: primary!,
            '&:hover': {
              backgroundColor: primary!,
              boxShadow: `0 0 0 2px ${alpha(highlight!, 0.55)} inset, 0 0 14px ${alpha(highlight!, 0.45)}`
            },
            '&:active': {
              backgroundColor: primary!,
              boxShadow: `0 0 0 2px ${alpha(highlight!, 0.65)} inset, 0 0 18px ${alpha(highlight!, 0.5)}`
            }
          },
          root: {
            ...btnRoot,

            '&.MuiButton-containedPrimary:focus-visible': {
              outline: 'none',
              boxShadow: `0 0 0 2px ${alpha(highlight!, 0.75)} inset, 0 0 18px ${alpha(highlight!, 0.65)}`
            },

            '&.hover-ring.MuiButton-containedPrimary:hover': {
              backgroundColor: primary!,
              boxShadow: `0 0 0 2px ${alpha(highlight!, 0.65)} inset, 0 0 16px ${alpha(highlight!, 0.55)}`
            },
            '&.hover-ring.MuiButton-containedPrimary:focus-visible': {
              outline: 'none',
              boxShadow: `0 0 0 2px ${alpha(highlight!, 0.85)} inset, 0 0 20px ${alpha(highlight!, 0.7)}`
            }
          }
        }
      },

      MuiIconButton: {
        defaultProps: {
          disableRipple: false,
          disableFocusRipple: false,
          disableTouchRipple: false
        },
        styleOverrides: {
          ...iconButtonSO,
          root: {
            ...iconBtnRoot,
            backgroundColor: 'transparent',
            boxShadow: 'none',
            outline: 'none',

            '&:hover': { backgroundColor: 'transparent' },
            '&.Mui-focusVisible': { backgroundColor: 'transparent' },
            '&.nav-focus-primary .MuiSvgIcon-root': {
              transition: 'color 120ms ease-out'
            },
            '&.nav-focus-primary:hover .MuiSvgIcon-root, &.nav-focus-primary.Mui-focusVisible .MuiSvgIcon-root':
              {
                color: primary!
              }
          }
        }
      },

      MuiSwitch: {
        defaultProps: {
          disableRipple: false,
          disableFocusRipple: false,
          disableTouchRipple: false
        },
        styleOverrides: {
          ...switchSO,
          switchBase: {
            ...swSwitchBase,

            '&.Mui-checked': {
              color: `${primary!} !important`
            },

            '&.Mui-checked + .MuiSwitch-track': {
              backgroundColor: `${primary!} !important`,
              opacity: 1
            },

            '&.Mui-focusVisible': {
              backgroundColor: 'transparent'
            }
          },
          thumb: { ...swThumb, boxShadow: 'none' },
          track: { ...swTrack, opacity: 1 }
        }
      },
      MuiSlider: {
        styleOverrides: {
          thumb: {
            '&.Mui-focusVisible, &.Mui-active, &:hover': {
              boxShadow: `0 0 0 10px ${alpha(highlight!, 0.75)}`
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
