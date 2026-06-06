import AccessTimeIcon from '@mui/icons-material/AccessTime'
import AppsIcon from '@mui/icons-material/Apps'
import NavigationOutlinedIcon from '@mui/icons-material/NavigationOutlined'
import PlaceIcon from '@mui/icons-material/Place'
import RouteIcon from '@mui/icons-material/Route'
import SignpostIcon from '@mui/icons-material/Signpost'
import { Box, Stack, Typography } from '@mui/material'
import type { NaviBag } from '@shared/types'

import { NavLocale, translateNavigation } from '@shared/utils/translateNavigation'
import { useLiviStore } from '@store/store'
import * as React from 'react'
import { CENTER_X, NAV_DIVIDER_Y, NAV_Y } from '../dashboards/constants'
import { ManeuverGraphic } from './ManeuverIcon'

export type NavFullProps = {
  className?: string
}

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

  // readNavigation() shape: { timestamp, payload: { ... } }
  if (isRecord(raw.payload)) {
    const p = raw.payload as Record<string, unknown>

    // persisted shape: { metaType, navi, display, ... }
    if (isRecord(p.navi)) return p.navi as unknown as Partial<NaviBag>

    if (
      'NaviStatus' in p ||
      'NaviAPPName' in p ||
      'NaviDestinationName' in p ||
      'NaviManeuverType' in p ||
      'NaviRemainDistance' in p
    ) {
      return p as unknown as Partial<NaviBag>
    }
  }

  // projection-event message wrapper
  if (isRecord(raw) && isRecord(raw.payload)) {
    const p = raw.payload as Record<string, unknown>

    if (isRecord(p.navi)) return p.navi as unknown as Partial<NaviBag>

    if (
      'NaviStatus' in p ||
      'NaviAPPName' in p ||
      'NaviDestinationName' in p ||
      'NaviManeuverType' in p ||
      'NaviRemainDistance' in p
    ) {
      return p as unknown as Partial<NaviBag>
    }
  }

  // { navi: {...} }
  if (isRecord((raw as Record<string, unknown>).navi)) {
    return (raw as Record<string, unknown>).navi as unknown as Partial<NaviBag>
  }

  if (
    'NaviStatus' in raw ||
    'NaviAPPName' in raw ||
    'NaviDestinationName' in raw ||
    'NaviManeuverType' in raw ||
    'NaviRemainDistance' in raw
  ) {
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

export function NavFull({ className }: NavFullProps) {
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
      if (patch) {
        setNavi((prev) => mergeNavi(prev, patch))
      } else {
        void hydrate()
      }
    }

    const unsubscribe = window.projection.ipc.onEvent(handler)
    return unsubscribe
  }, [hydrate])

  const t = React.useMemo(() => translateNavigation(navi, locale), [navi, locale])

  const remainDistanceText =
    isRecord(t) && typeof t.RemainDistanceText === 'string' ? t.RemainDistanceText : undefined

  const isActive = navi?.NaviStatus === 1

  const maneuverImageBase64 =
    typeof navi?.NaviImageBase64 === 'string' && navi.NaviImageBase64.length > 0
      ? navi.NaviImageBase64
      : undefined

  const hasManeuverImage = Boolean(maneuverImageBase64)

  const maneuverText =
    maneuverImageBase64 && t.ManeuverTypeText === 'Unknown' ? undefined : t.ManeuverTypeText

  if (!isActive) {
    // No guidance → a single nav glyph centred on NAV_Y
    return (
      <Box
        className={className}
        sx={{
          position: 'absolute',
          left: CENTER_X,
          top: NAV_Y,
          transform: 'translate(-50%, -50%)',
          pointerEvents: 'none',
          display: 'grid',
          placeItems: 'center'
        }}
      >
        <NavigationOutlinedIcon sx={{ fontSize: 84, opacity: 0.55 }} />
      </Box>
    )
  }

  // Maneuver block that sits above the divider (icon + distance, with the maneuver text below or
  // beside it depending on whether the phone supplied a maneuver image).
  const maneuverBlock = hasManeuverImage ? (
    <Stack spacing={1.2} sx={{ alignItems: 'center', justifyContent: 'center' }}>
      <Box sx={{ display: 'flex', flexDirection: 'column', alignItems: 'center', minWidth: 92 }}>
        <ManeuverGraphic
          imageBase64={maneuverImageBase64}
          type={t.codes.ManeuverType}
          turnSide={t.codes.TurnSide}
          size={72}
        />

        {remainDistanceText && (
          <Typography
            sx={{ mt: 0.5, fontSize: 20, fontWeight: 600, letterSpacing: 0.2, lineHeight: 1 }}
          >
            {remainDistanceText}
          </Typography>
        )}
      </Box>

      {maneuverText && (
        <Typography variant="h5" sx={{ lineHeight: 1.1, textAlign: 'center' }}>
          {maneuverText}
        </Typography>
      )}
    </Stack>
  ) : (
    <Stack direction="row" spacing={2} sx={{ alignItems: 'center', justifyContent: 'center' }}>
      <Box sx={{ display: 'flex', flexDirection: 'column', alignItems: 'center', minWidth: 92 }}>
        <ManeuverGraphic
          imageBase64={maneuverImageBase64}
          type={t.codes.ManeuverType}
          turnSide={t.codes.TurnSide}
          size={72}
        />

        {remainDistanceText && (
          <Typography
            sx={{ mt: 0.5, fontSize: 20, fontWeight: 600, letterSpacing: 0.2, lineHeight: 1 }}
          >
            {remainDistanceText}
          </Typography>
        )}
      </Box>

      <Box sx={{ minWidth: 0, textAlign: 'left' }}>
        {maneuverText && (
          <Typography variant="h5" sx={{ lineHeight: 1.1 }}>
            {maneuverText}
          </Typography>
        )}

        {t.CurrentRoadName && (
          <Stack direction="row" spacing={1} component="div" sx={{ mt: 0.6, alignItems: 'center' }}>
            <SignpostIcon fontSize="small" sx={{ opacity: 0.85 }} />
            <Typography variant="body2" sx={{ opacity: 0.85 }} noWrap>
              {t.CurrentRoadName}
            </Typography>
          </Stack>
        )}
      </Box>
    </Stack>
  )

  return (
    <Box
      className={className}
      sx={{
        position: 'absolute',
        left: CENTER_X,
        top: NAV_DIVIDER_Y,
        transform: 'translateX(-50%)',
        // Size to the content (the info row) so the divider matches it, not a fixed 920px band.
        width: 'fit-content',
        maxWidth: 'min(920px, 100%)',
        pointerEvents: 'none'
      }}
    >
      {/* Above the divider, anchored upward so the divider line stays fixed at NAV_DIVIDER_Y. */}
      <Box
        sx={{
          position: 'absolute',
          bottom: '100%',
          left: '50%',
          transform: 'translateX(-50%)',
          pb: 2.6,
          display: 'flex',
          justifyContent: 'center'
        }}
      >
        {maneuverBlock}
      </Box>

      {/* Divider line at NAV_DIVIDER_Y (matches the mini nav). */}
      <Box
        sx={{
          width: hasManeuverImage ? '72%' : '100%',
          mx: 'auto',
          height: 1.4,
          borderRadius: 999,
          bgcolor: 'text.secondary',
          opacity: 0.35
        }}
      />

      {/* Below the divider: ETA / remaining distance / destination row. */}
      <Stack
        direction="row"
        spacing={3}
        sx={{
          pt: 2.6,
          flexWrap: 'wrap',
          justifyContent: 'center',
          rowGap: 1
        }}
      >
        {hasManeuverImage && t.CurrentRoadName && (
          <Stack
            direction="row"
            spacing={1}
            sx={{
              alignItems: 'center',
              minWidth: 0
            }}
          >
            <SignpostIcon fontSize="small" sx={{ opacity: 0.85 }} />
            <Typography variant="body2" sx={{ opacity: 0.85 }} noWrap>
              {t.CurrentRoadName}
            </Typography>
          </Stack>
        )}

        {t.TimeRemainingToDestinationText && (
          <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
            <AccessTimeIcon fontSize="small" />
            <Typography variant="body1">{t.TimeRemainingToDestinationText}</Typography>
          </Stack>
        )}

        {t.DistanceRemainingDisplayStringText && (
          <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
            <RouteIcon fontSize="small" />
            <Typography variant="body1">{t.DistanceRemainingDisplayStringText}</Typography>
          </Stack>
        )}

        {t.DestinationName && (
          <Stack
            direction="row"
            spacing={1}
            sx={{
              alignItems: 'center',
              minWidth: 0
            }}
          >
            <PlaceIcon fontSize="small" />
            <Typography variant="body1" noWrap sx={{ minWidth: 0 }}>
              {t.DestinationName}
            </Typography>
          </Stack>
        )}

        {t.SourceName && (
          <Stack
            direction="row"
            spacing={1}
            sx={{
              alignItems: 'center',
              minWidth: 0
            }}
          >
            <AppsIcon fontSize="small" />
            <Typography variant="body2" sx={{ opacity: 0.85 }} noWrap>
              {t.SourceName}
            </Typography>
          </Stack>
        )}
      </Stack>
    </Box>
  )
}
