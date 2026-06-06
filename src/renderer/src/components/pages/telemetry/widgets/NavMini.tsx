import AccessTimeIcon from '@mui/icons-material/AccessTime'
import RouteIcon from '@mui/icons-material/Route'
import SignpostIcon from '@mui/icons-material/Signpost'
import { Box, Typography, useTheme } from '@mui/material'
import type { NaviBag } from '@shared/types'
import { NavLocale, translateNavigation } from '@shared/utils/translateNavigation'

import { useLiviStore } from '@store/store'
import * as React from 'react'
import { CENTER_X, NAV_DIVIDER_Y, NAV_Y } from '../dashboards/constants'
import { Clock } from './Clock'
import { ManeuverGraphic } from './ManeuverIcon'

type ProjectionEventMsg = { type: string; payload?: unknown }

function navLocaleFromSettings(v: unknown): NavLocale {
  if (v === 'de' || v === 'ua' || v === 'en') return v
  return 'en'
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null
}

function unwrapNaviPatch(raw: unknown): Partial<NaviBag> | null {
  if (!isRecord(raw)) return null

  if (isRecord(raw.payload)) {
    const p = raw.payload as Record<string, unknown>
    if (isRecord(p.navi)) return p.navi as unknown as Partial<NaviBag>

    if ('NaviStatus' in p || 'NaviManeuverType' in p || 'NaviRemainDistance' in p) {
      return p as unknown as Partial<NaviBag>
    }
  }

  if (isRecord((raw as Record<string, unknown>).navi)) {
    return (raw as Record<string, unknown>).navi as unknown as Partial<NaviBag>
  }

  if ('NaviStatus' in raw || 'NaviManeuverType' in raw || 'NaviRemainDistance' in raw) {
    return raw as unknown as Partial<NaviBag>
  }

  return null
}

function mergeNavi(prev: NaviBag | null, patch: Partial<NaviBag> | null): NaviBag | null {
  if (!patch) return prev
  if (!prev) return patch as NaviBag
  return {
    ...(prev as unknown as Record<string, unknown>),
    ...(patch as unknown as Record<string, unknown>)
  } as NaviBag
}

export type NavMiniProps = {
  className?: string
  iconSize?: number
}

/**
 * Mini widget layout (divider anchored at NAV_DIVIDER_Y, same line as the full nav):
 * - Maneuver icon + RemainDistanceText, anchored above the divider
 * - divider
 * - bottom row: ETA + remaining distance
 * - if no route data => centered clock only (on NAV_Y)
 */
export function NavMini({ className, iconSize = 56 }: NavMiniProps) {
  const theme = useTheme()
  const settings = useLiviStore((s) => s.settings)
  const locale = navLocaleFromSettings(settings?.language)

  const [navi, setNavi] = React.useState<NaviBag | null>(null)

  const hydrate = React.useCallback(async () => {
    try {
      const snap = await window.projection.ipc.readNavigation()
      const patch = unwrapNaviPatch(snap)
      setNavi((prev) => mergeNavi(prev, patch))
    } catch {
      // keep previous state
    }
  }, [])

  React.useEffect(() => {
    void hydrate()

    const handler = (_event: unknown, ...args: unknown[]) => {
      const msg = (args[0] ?? {}) as ProjectionEventMsg
      if (msg.type === 'plugged') {
        void hydrate()
        return
      }
      if (msg.type === 'unplugged') {
        setNavi(null)
        return
      }
      if (msg.type !== 'navigation') return

      const patch = unwrapNaviPatch(msg)
      if (patch) setNavi((prev) => mergeNavi(prev, patch))
      else void hydrate()
    }

    const unsubscribe = window.projection.ipc.onEvent(handler)
    return unsubscribe
  }, [hydrate])

  const t = React.useMemo(() => translateNavigation(navi, locale), [navi, locale])
  const isActive = navi?.NaviStatus === 1
  const maneuverType = t.codes.ManeuverType
  const turnSide = t.codes.TurnSide
  const remainDistanceText = t.RemainDistanceText
  const maneuverText =
    typeof t.ManeuverTypeText === 'string' && t.ManeuverTypeText !== 'Unknown'
      ? t.ManeuverTypeText
      : undefined
  const etaText = t.TimeRemainingToDestinationText
  const destinationDistanceText = t.DistanceRemainingDisplayStringText
  const distanceLineText =
    remainDistanceText && remainDistanceText !== '—' ? remainDistanceText : (maneuverText ?? '—')

  const maneuverImageBase64 =
    typeof navi?.NaviImageBase64 === 'string' && navi.NaviImageBase64.length > 0
      ? navi.NaviImageBase64
      : undefined

  const hasManeuverImage = Boolean(maneuverImageBase64)

  const bottomLeftText =
    etaText && etaText !== '—'
      ? etaText
      : typeof t.CurrentRoadName === 'string' && t.CurrentRoadName.length > 0
        ? t.CurrentRoadName
        : '—'

  if (!isActive) {
    // No route guidance → just the clock, centred on NAV_Y.
    return (
      <Box
        sx={{
          position: 'absolute',
          left: CENTER_X,
          top: NAV_Y,
          transform: 'translate(-50%, -50%)',
          width: 280,
          height: 120,
          display: 'grid',
          placeItems: 'center'
        }}
      >
        <Clock className={className} />
      </Box>
    )
  }

  return (
    <Box
      className={className}
      sx={{
        position: 'absolute',
        left: CENTER_X,
        top: NAV_DIVIDER_Y,
        transform: 'translateX(-50%)',
        width: 280,
        minWidth: 0
      }}
    >
      {/* Icon + distance, anchored above the divider so the divider stays fixed at NAV_DIVIDER_Y. */}
      <Box
        sx={{
          position: 'absolute',
          bottom: '100%',
          left: 0,
          right: 0,
          pb: 1.6,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          color: theme.palette.text.primary
        }}
      >
        <Box sx={{ display: 'grid', placeItems: 'center' }}>
          <ManeuverGraphic
            imageBase64={maneuverImageBase64}
            type={maneuverType}
            turnSide={turnSide}
            size={iconSize}
          />
        </Box>

        <Typography
          sx={{
            mt: 1.6,
            fontSize: 22,
            fontWeight: 600,
            lineHeight: 1,
            textAlign: 'center',
            whiteSpace: 'nowrap'
          }}
        >
          {distanceLineText}
        </Typography>
      </Box>

      {/* Divider line at NAV_DIVIDER_Y (matches the full nav). */}
      <Box
        sx={{
          width: hasManeuverImage ? '72%' : '100%',
          mx: 'auto',
          height: 1.4,
          borderRadius: 999,
          bgcolor: theme.palette.text.secondary,
          opacity: 0.35
        }}
      />

      {/* Bottom row: ETA / road + remaining distance. */}
      <Box
        sx={{
          pt: 1.6,
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: hasManeuverImage ? 'center' : 'space-between',
          gap: 2.2,
          whiteSpace: 'nowrap'
        }}
      >
        <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.7, minWidth: 0 }}>
          {etaText && etaText !== '—' ? (
            <AccessTimeIcon sx={{ fontSize: 22, opacity: 0.9 }} />
          ) : (
            <SignpostIcon sx={{ fontSize: 22, opacity: 0.9 }} />
          )}

          <Typography
            sx={{
              fontSize: 20,
              fontWeight: 500,
              lineHeight: 1.4,
              whiteSpace: 'nowrap',
              fontVariantNumeric: 'tabular-nums',
              minWidth: 0,
              overflow: 'hidden',
              textOverflow: 'ellipsis'
            }}
          >
            {bottomLeftText}
          </Typography>
        </Box>

        {!hasManeuverImage && (
          <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.7 }}>
            <RouteIcon sx={{ fontSize: 22, opacity: 0.9 }} />
            <Typography
              sx={{
                fontSize: 22,
                fontWeight: 500,
                lineHeight: 1,
                whiteSpace: 'nowrap',
                fontVariantNumeric: 'tabular-nums'
              }}
            >
              {destinationDistanceText ?? '—'}
            </Typography>
          </Box>
        )}
      </Box>
    </Box>
  )
}
