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
    return s || '—'
  }
  return '—'
}

const toStringOrDash = (v: unknown): string => {
  if (v == null) return '—'
  if (typeof v === 'string') return v.trim() || '—'
  if (typeof v === 'number' || typeof v === 'boolean') return String(v)
  return '—'
}

export const About = () => {
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
    const appContrib = contributorsStr || '—'

    return [
      { label: 'Name', value: appName },
      { label: 'Description', value: appDesc, tooltip: appDesc },
      { label: 'Version', value: appVersion, mono: true },
      { label: 'Build', value: appVersion, mono: true },
      { label: 'URL', value: appHomepage, tooltip: appHomepage },
      { label: 'Author', value: appAuthor, tooltip: appAuthor },
      { label: 'Contributors', value: appContrib, tooltip: appContrib }
    ]
  }, [contributorsStr])

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
