import { useMemo } from 'react'
import { Box, Stack, Typography } from '@mui/material'
import {
  name,
  description,
  version,
  author,
  contributors,
  homepage
} from '../../../../../../../../package.json'
import { useTranslation } from 'react-i18next'
import { EMPTY_STRING } from '@renderer/constants'

type Row = {
  label: string
  value: string
  mono?: boolean
  tooltip?: string
}

const isNonEmptyString = (v: unknown): v is string => typeof v === 'string' && v.trim().length > 0

const toAuthorString = (a: unknown): string => {
  if (isNonEmptyString(a)) return a
  if (a && typeof a === 'object') {
    const anyA = a as any
    const n = isNonEmptyString(anyA.name) ? anyA.name : ''
    const e = isNonEmptyString(anyA.email) ? `<${anyA.email}>` : ''
    const u = isNonEmptyString(anyA.url) ? `(${anyA.url})` : ''
    const s = [n, e, u].filter(Boolean).join(' ')
    return s || EMPTY_STRING
  }
  return EMPTY_STRING
}

const toStringOrDash = (v: unknown): string => {
  if (v == null) return EMPTY_STRING
  if (typeof v === 'string') return v.trim() || EMPTY_STRING
  if (typeof v === 'number' || typeof v === 'boolean') return String(v)
  return EMPTY_STRING
}

export const About = () => {
  const { t } = useTranslation()
  const contributorsStr = useMemo(() => {
    if (Array.isArray(contributors) && contributors.length > 0) {
      return contributors
        .map((c) => (typeof c === 'string' ? c : (c as any)?.name))
        .filter(isNonEmptyString)
        .join(', ')
        .trim()
    }
    return ''
  }, [])

  const rows = useMemo<Row[]>(() => {
    const appName = toStringOrDash(name)
    const appDesc = toStringOrDash(description)
    const appVersion = toStringOrDash(version)
    const appHomepage = toStringOrDash(homepage)
    const appAuthor = toAuthorString(author)
    const appContrib = contributorsStr || EMPTY_STRING

    return [
      { label: t('settings.name'), value: appName },
      { label: t('settings.description'), value: appDesc, tooltip: appDesc },
      { label: t('settings.version'), value: appVersion, mono: true },
      { label: t('settings.build'), value: appVersion, mono: true },
      { label: t('settings.url'), value: appHomepage, tooltip: appHomepage },
      { label: t('settings.author'), value: appAuthor, tooltip: appAuthor },
      { label: t('settings.contributors'), value: appContrib, tooltip: appContrib }
    ]
  }, [contributorsStr, t])

  const Mono: React.CSSProperties = {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
    fontVariantNumeric: 'tabular-nums'
  }

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Stack spacing={0.75}>
        {rows.map((r) => (
          <Stack key={r.label} direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
            <Typography sx={{ minWidth: 110 }} color="text.secondary">
              {r.label}:
            </Typography>

            <Typography
              title={r.tooltip ?? r.value}
              sx={{
                ...(r.mono ? Mono : null),
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                maxWidth: '42ch'
              }}
            >
              {r.value}
            </Typography>
          </Stack>
        ))}
      </Stack>
    </Box>
  )
}
