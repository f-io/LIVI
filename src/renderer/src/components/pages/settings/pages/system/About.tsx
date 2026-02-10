import { useMemo } from 'react'
import type { CSSProperties } from 'react'
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

const contributorsValue: unknown = contributors as unknown

const isNonEmptyString = (v: unknown): v is string => typeof v === 'string' && v.trim().length > 0

type PersonLike = {
  name?: unknown
  email?: unknown
  url?: unknown
}

const isPersonLike = (v: unknown): v is PersonLike => typeof v === 'object' && v !== null

const toAuthorString = (a: unknown): string => {
  if (isNonEmptyString(a)) return a

  if (isPersonLike(a)) {
    const n = isNonEmptyString(a.name) ? a.name : ''
    const e = isNonEmptyString(a.email) ? `<${a.email}>` : ''
    const u = isNonEmptyString(a.url) ? `(${a.url})` : ''
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
    const list = Array.isArray(contributorsValue) ? (contributorsValue as unknown[]) : []
    if (list.length > 0) {
      return list
        .map((c) => {
          if (typeof c === 'string') return c
          if (isPersonLike(c) && isNonEmptyString(c.name)) return c.name
          return ''
        })
        .filter(isNonEmptyString)
        .join(', ')
        .trim()
    }
    return ''
  }, [])

  // Build metadata injected by electron.vite.config.ts
  const buildRunStr = useMemo(() => {
    const run = __BUILD_RUN__?.trim?.() ? __BUILD_RUN__.trim() : ''
    return run ? `#${run}` : EMPTY_STRING
  }, [])

  const commitShaStr = useMemo(() => {
    const sha = __BUILD_SHA__?.trim?.() ? __BUILD_SHA__.trim() : 'dev'
    return sha
  }, [])

  const rows = useMemo<Row[]>(() => {
    const appName = toStringOrDash(name)
    const appDesc = toStringOrDash(description)
    const appVersion = toStringOrDash(version)
    const appHomepage = toStringOrDash(homepage)
    const appAuthor = toAuthorString(author)
    const appContrib = contributorsStr || EMPTY_STRING

    const out: Row[] = [
      { label: t('settings.name'), value: appName },
      { label: t('settings.description'), value: appDesc, tooltip: appDesc },
      { label: t('settings.version'), value: appVersion, mono: true }
    ]

    if (buildRunStr) {
      out.push({ label: t('settings.build'), value: buildRunStr, mono: true })
    }

    out.push({ label: t('settings.commit', 'Commit'), value: commitShaStr, mono: true })

    out.push(
      { label: t('settings.url'), value: appHomepage, tooltip: appHomepage },
      { label: t('settings.author'), value: appAuthor, tooltip: appAuthor },
      { label: t('settings.contributors'), value: appContrib, tooltip: appContrib }
    )

    return out
  }, [contributorsStr, t, buildRunStr, commitShaStr])

  const Mono: CSSProperties = {
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
