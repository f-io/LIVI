import type { Config } from '@shared/types'

/** Defaults mirror renderer/theme/themeColors. */
const FALLBACK = {
  day: {
    background: '#d4d4d4',
    text: '#000000',
    textSecondary: '#333333',
    primary: '#008585',
    highlight: '#007575',
    divider: '#cccccc'
  },
  night: {
    background: '#000000',
    text: '#ffffff',
    textSecondary: '#bbbbbb',
    primary: '#00adad',
    highlight: '#009494',
    divider: '#444444'
  }
}

/** LIVI's colours as custom properties, so a page in the custom folder follows the
 *  head unit instead of carrying its own palette. */
export function themeCss(cfg: Partial<Config> | undefined, dark: boolean): string {
  const base = dark ? FALLBACK.night : FALLBACK.day
  const pick = (value: string | undefined, fallback: string): string =>
    value && value.trim() ? value : fallback

  const vars = {
    '--livi-background': pick(
      dark ? cfg?.backgroundColorDark : cfg?.backgroundColorLight,
      base.background
    ),
    '--livi-text': base.text,
    '--livi-text-secondary': base.textSecondary,
    '--livi-primary': pick(dark ? cfg?.primaryColorDark : cfg?.primaryColorLight, base.primary),
    '--livi-highlight': pick(
      dark ? cfg?.highlightColorDark : cfg?.highlightColorLight,
      base.highlight
    ),
    '--livi-divider': base.divider
  }

  const body = Object.entries(vars)
    .map(([k, v]) => `  ${k}: ${v};`)
    .join('\n')

  return `:root {\n  color-scheme: ${dark ? 'dark' : 'light'};\n${body}\n}\n`
}
