import { alpha, createTheme } from '@mui/material/styles'
import { CSSObject } from '@mui/system'
import { SliderValueThumb } from './components/SliderValueThumb'
import { THEME, UI } from './constants'
import { themeColors } from './themeColors'

const sliderPill = {
  medium: { size: 'clamp(34px, 6svh, 46px)', font: 'clamp(0.95rem, 2.5svh, 1.2rem)' },
  small: { size: 'clamp(26px, 4.6svh, 34px)', font: 'clamp(0.75rem, 2svh, 0.95rem)' }
}

type SliderStyleArgs = { ownerState: { size?: string; valueLabelDisplay?: string } }

const glowColor = (percent: number): string =>
  `color-mix(in srgb, currentColor ${percent}%, transparent)`

const asCssRecord = (v: unknown): Record<string, CSSObject> => v as Record<string, CSSObject>
const asUnknownRecord = (v: unknown): Record<string, unknown> => v as Record<string, unknown>
const asCss = (v: unknown): CSSObject => v as CSSObject

function sliderComponent(primary: string, isLight: boolean) {
  return {
    defaultProps: {
      valueLabelDisplay: 'on' as const,
      slots: { thumb: SliderValueThumb }
    },
    styleOverrides: {
      root: ({ ownerState }: SliderStyleArgs): CSSObject => {
        if (ownerState.valueLabelDisplay === 'off') return {}
        const pill = ownerState.size === 'small' ? sliderPill.small : sliderPill.medium
        return {
          height: pill.size,
          padding: 0,
          '& .MuiSlider-rail': {
            height: pill.size,
            borderRadius: '999px',
            opacity: 1,
            backgroundColor: `color-mix(in srgb, currentColor ${isLight ? 18 : 26}%, transparent)`,
            boxShadow: `0 0 4px 0 ${glowColor(18)}`,
            transition: 'box-shadow 140ms ease-out'
          },
          '&:hover .MuiSlider-rail, &:has(.MuiSlider-thumb.Mui-active) .MuiSlider-rail, &:has(.MuiSlider-thumb.Mui-focusVisible) .MuiSlider-rail':
            {
              boxShadow: `0 0 8px 0 ${glowColor(35)}`
            },
          '& .MuiSlider-track': {
            height: pill.size,
            border: 0,
            borderRadius: '999px',
            transition: 'none'
          },
          '& .MuiSlider-thumb': {
            height: pill.size,
            width: 0,
            minWidth: 0,
            padding: 0,
            borderRadius: 0,
            backgroundColor: 'transparent',
            boxShadow: 'none',
            transition: 'none',
            '&::before': { display: 'none' },
            '&::after': { display: 'none' },
            '&.Mui-focusVisible, &.Mui-active, &:hover': { boxShadow: 'none' },
            '& .LiviSlider-value': {
              position: 'absolute',
              top: '50%',
              transform: 'translateY(-50%)',
              whiteSpace: 'nowrap',
              display: 'flex',
              alignItems: 'center',
              fontSize: pill.font,
              fontWeight: 900,
              lineHeight: 1,
              right: '1em',
              '&[data-inside="true"]': { color: '#fff' },
              '&[data-inside="false"]': {
                color: isLight ? themeColors.textPrimaryLight : themeColors.textPrimaryDark
              }
            },
            '& .MuiSvgIcon-root': { fontSize: '1.3em' }
          },
          '& .MuiSlider-valueLabel': { display: 'none' },
          '& .MuiSlider-mark': { display: 'none' },
          '& .MuiSlider-markLabel': { top: `calc(${pill.size} + 8px)` }
        }
      }
    }
  }
}

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
  '& svg': { color: 'inherit' },
  '&.Mui-selected svg': { color: 'inherit' }
}
const buttonBaseRoot = { cursor: 'default' }
const svgIconRoot = { cursor: 'default' }

function buildTheme(mode: THEME.LIGHT | THEME.DARK, bg?: string) {
  const isLight = mode === THEME.LIGHT
  const primary = isLight ? themeColors.primaryColorLight : themeColors.primaryColorDark
  const highlight = isLight ? themeColors.highlightColorLight : themeColors.highlightColorDark
  const background = bg || (isLight ? themeColors.light : themeColors.dark)
  const surface = isLight ? themeColors.surfaceLight : themeColors.surfaceDark
  const raised = isLight ? themeColors.raisedLight : themeColors.raisedDark
  const border = isLight ? themeColors.borderLight : themeColors.borderDark

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
    typography: {
      fontFamily:
        'Bahnschrift, "DIN Alternate", "D-DIN", "DIN 2014", Inter, Roboto, "Helvetica Neue", Arial, sans-serif'
    },
    palette: {
      mode,
      background: {
        default: background,
        paper: background
      },
      text: {
        primary: isLight ? themeColors.textPrimaryLight : themeColors.textPrimaryDark,
        secondary: isLight ? themeColors.textSecondaryLight : themeColors.textSecondaryDark
      },
      primary: { main: primary },
      secondary: { main: highlight },
      divider: isLight ? themeColors.dividerLight : themeColors.dividerDark,
      success: { main: themeColors.successMain },
      warning: { main: themeColors.warningMain },
      error: { main: themeColors.errorMain },
      action: {
        disabled: isLight ? themeColors.disabledLight : themeColors.disabledDark
      }
    },
    components: {
      MuiCssBaseline: {
        styleOverrides: {
          ...commonLayout,
          body: {
            backgroundColor: background,
            '--ui-highlight': highlight,
            '--ui-primary': primary,
            '--ui-surface': surface,
            '--ui-raised': raised,
            '--ui-border': border,
            '--ui-breathe-dur': '1350ms'
          },
          '.fft-surface': {
            backgroundColor: 'transparent'
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
            backgroundColor: background
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

      MuiSlider: sliderComponent(primary, isLight),

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
        variants: [
          {
            props: { variant: 'contained', color: 'primary' },
            style: {
              backgroundColor: primary,
              '&:hover': {
                backgroundColor: primary,
                boxShadow: `0 0 0 2px ${alpha(highlight, 0.55)} inset, 0 0 14px ${alpha(highlight, 0.45)}`
              },
              '&:active': {
                backgroundColor: primary,
                boxShadow: `0 0 0 2px ${alpha(highlight, 0.65)} inset, 0 0 18px ${alpha(highlight, 0.5)}`
              }
            }
          }
        ],
        styleOverrides: {
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
            backgroundColor: background,
            boxShadow: 'none'
          }
        }
      },

      MuiCard: {
        styleOverrides: {
          root: {
            borderRadius: 8,
            backgroundColor: surface,
            border: `1px solid ${border}`,
            boxShadow: isLight ? '0 2px 8px rgba(0,0,0,0.1)' : '0 2px 10px rgba(0,0,0,0.5)'
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
  highlight?: string,
  background?: string
) {
  if (!primary && !highlight && !background) return buildTheme(mode)

  const base = buildTheme(mode, background)

  if (!primary) {
    primary = mode === THEME.LIGHT ? themeColors.primaryColorLight : themeColors.primaryColorDark
  }
  if (!highlight) {
    highlight =
      mode === THEME.LIGHT ? themeColors.highlightColorLight : themeColors.highlightColorDark
  }

  const tabsSO = asCssRecord(base.components?.MuiTabs?.styleOverrides)
  const outlinedSO = asCssRecord(base.components?.MuiOutlinedInput?.styleOverrides)
  const inputLabelSO = asCssRecord(base.components?.MuiInputLabel?.styleOverrides)
  const buttonSO = asCssRecord(base.components?.MuiButton?.styleOverrides)
  const buttonBaseSO = asCssRecord(base.components?.MuiButtonBase?.styleOverrides)
  const iconButtonSO = asCssRecord(base.components?.MuiIconButton?.styleOverrides)
  const switchSO = asCssRecord(base.components?.MuiSwitch?.styleOverrides)
  const cssBaselineSO = asUnknownRecord(base.components?.MuiCssBaseline?.styleOverrides)

  const cssBodySO = asCss(cssBaselineSO.body)

  const tabsIndicator = asCss(tabsSO.indicator)
  const outlinedRoot = asCss(outlinedSO.root)
  const outlinedNotched = asCss(outlinedSO.notchedOutline)
  const inputLabelRoot = asCss(inputLabelSO.root)
  const btnRoot = asCss(buttonSO.root)
  const btnBaseRoot = asCss(buttonBaseSO.root)

  const iconBtnRoot = asCss(iconButtonSO.root)
  const swSwitchBase = asCss(switchSO.switchBase)
  const swThumb = asCss(switchSO.thumb)
  const swTrack = asCss(switchSO.track)

  return createTheme({
    ...base,
    palette: {
      ...base.palette,
      primary: { main: primary! },
      secondary: { main: highlight! }
    },

    components: {
      ...base.components,

      MuiCssBaseline: {
        styleOverrides: {
          ...cssBaselineSO,
          body: {
            ...cssBodySO,
            '--ui-highlight': highlight!,
            '--ui-primary': primary!
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
        variants: [
          {
            props: { variant: 'contained', color: 'primary' },
            style: {
              backgroundColor: primary!,
              '&:hover': {
                backgroundColor: primary!,
                boxShadow: `0 0 0 2px ${alpha(highlight!, 0.55)} inset, 0 0 14px ${alpha(highlight!, 0.45)}`
              },
              '&:active': {
                backgroundColor: primary!,
                boxShadow: `0 0 0 2px ${alpha(highlight!, 0.65)} inset, 0 0 18px ${alpha(highlight!, 0.5)}`
              }
            }
          }
        ],
        styleOverrides: {
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
      MuiSlider: sliderComponent(primary!, mode === THEME.LIGHT)
    }
  })
}

export function initCursorHider() {
  const inactivityMs = UI.INACTIVITY_HIDE_DELAY_MS
  let timer: ReturnType<typeof setTimeout>
  let lastX: number | null = null
  let lastY: number | null = null
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
  // Touch emits synthetic mouse events, only a real mouse reveals the pointer
  document.addEventListener('pointermove', (e) => {
    window.app?.notifyUserActivity?.()
    if (e.pointerType !== 'mouse') return
    const moved = lastX !== null && (e.clientX !== lastX || e.clientY !== lastY)
    lastX = e.clientX
    lastY = e.clientY
    if (moved) reset()
  })
  setCursor('none')
}

// CarPlay-style LED
let started = false
export function initUiBreatheClock() {
  if (started) return
  started = true

  const root = document.documentElement

  const dur = 1600
  const min = 0.18
  const max = 1
  const range = max - min

  const start = performance.now()

  function tick() {
    const t = (performance.now() - start) % dur
    const p = t / dur

    let wave: number

    if (p < 0.35) {
      wave = p / 0.35
    } else if (p < 0.5) {
      wave = 1
    } else if (p < 0.85) {
      wave = 1 - (p - 0.5) / 0.35
    } else {
      wave = 0
    }

    const opacity = min + range * wave
    root.style.setProperty('--ui-breathe-opacity', opacity.toFixed(3))

    setTimeout(tick, 42) // ~24fps (very easy for RPi)
  }

  tick()
}
