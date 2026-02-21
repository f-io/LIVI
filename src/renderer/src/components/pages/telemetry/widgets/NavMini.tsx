import * as React from 'react'
import { Box, Typography, useTheme } from '@mui/material'

import AccessTimeIcon from '@mui/icons-material/AccessTime'
import RouteIcon from '@mui/icons-material/Route'
import { useBlinkingTime } from '../../../../hooks/useBlinkingTime'
import StraightIcon from '@mui/icons-material/Straight'
import TurnLeftIcon from '@mui/icons-material/TurnLeft'
import TurnRightIcon from '@mui/icons-material/TurnRight'
import TurnSlightLeftIcon from '@mui/icons-material/TurnSlightLeft'
import TurnSlightRightIcon from '@mui/icons-material/TurnSlightRight'
import TurnSharpLeftIcon from '@mui/icons-material/TurnSharpLeft'
import TurnSharpRightIcon from '@mui/icons-material/TurnSharpRight'
import SubdirectoryArrowLeftIcon from '@mui/icons-material/SubdirectoryArrowLeft'
import SubdirectoryArrowRightIcon from '@mui/icons-material/SubdirectoryArrowRight'
import UTurnLeftIcon from '@mui/icons-material/UTurnLeft'
import UTurnRightIcon from '@mui/icons-material/UTurnRight'
import RoundaboutRightIcon from '@mui/icons-material/RoundaboutRight'
import ForkLeftIcon from '@mui/icons-material/ForkLeft'
import ForkRightIcon from '@mui/icons-material/ForkRight'
import MergeIcon from '@mui/icons-material/Merge'
import ExitToAppIcon from '@mui/icons-material/ExitToApp'
import FlagIcon from '@mui/icons-material/Flag'
import HelpOutlineIcon from '@mui/icons-material/HelpOutline'

import type { NaviBag } from '../../../../../../main/carplay/services/types'
import { translateNavigation } from '../../../../../../main/carplay/services/utils/translateNavigation'
import type { NavLocale } from '../../../../../../main/carplay/services/utils/translateNavigation'
import { useCarplayStore } from '../../../../store/store'

type CarplayEventMsg = { type: string; payload?: unknown }

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

function ManeuverIcon({
  type,
  turnSide,
  size
}: {
  type: number | undefined
  turnSide: number | undefined
  size: number
}) {
  const isRight = turnSide === 2

  if (type == null) return <StraightIcon sx={{ fontSize: size }} />

  // roundabout exits 28..46
  if (type >= 28 && type <= 46) return <RoundaboutRightIcon sx={{ fontSize: size }} />

  switch (type) {
    case 0:
    case 3:
    case 5:
      return <StraightIcon sx={{ fontSize: size }} />

    case 1:
      return <TurnLeftIcon sx={{ fontSize: size }} />
    case 2:
      return <TurnRightIcon sx={{ fontSize: size }} />

    case 4:
    case 18:
    case 26:
      return isRight ? (
        <UTurnRightIcon sx={{ fontSize: size }} />
      ) : (
        <UTurnLeftIcon sx={{ fontSize: size }} />
      )

    case 6:
    case 7:
    case 19:
      return <RoundaboutRightIcon sx={{ fontSize: size }} />

    case 8:
    case 22:
    case 23:
      return <ExitToAppIcon sx={{ fontSize: size }} />

    case 9:
      return <MergeIcon sx={{ fontSize: size }} />

    case 10:
    case 12:
    case 24:
    case 25:
    case 27:
      return <FlagIcon sx={{ fontSize: size }} />

    case 13:
      return <ForkLeftIcon sx={{ fontSize: size }} />
    case 14:
      return <ForkRightIcon sx={{ fontSize: size }} />

    case 20:
      return <SubdirectoryArrowLeftIcon sx={{ fontSize: size }} />
    case 21:
      return <SubdirectoryArrowRightIcon sx={{ fontSize: size }} />

    case 47:
      return <TurnSharpLeftIcon sx={{ fontSize: size }} />
    case 48:
      return <TurnSharpRightIcon sx={{ fontSize: size }} />

    case 49:
      return <TurnSlightLeftIcon sx={{ fontSize: size }} />
    case 50:
      return <TurnSlightRightIcon sx={{ fontSize: size }} />

    case 52:
      return <ForkLeftIcon sx={{ fontSize: size }} />
    case 53:
      return <ForkRightIcon sx={{ fontSize: size }} />

    default:
      return <HelpOutlineIcon sx={{ fontSize: size, opacity: 0.8 }} />
  }
}

export type NavMiniProps = {
  className?: string
  iconSize?: number
}

/**
 * Mini widget layout:
 * - Maneuver icon
 * - RemainDistanceText
 * - divider
 * - bottom row: ETA + remaining distance
 * - if no route data => bottom row shows centered clock only
 */
export function NavMini({ className, iconSize = 56 }: NavMiniProps) {
  const theme = useTheme()
  const settings = useCarplayStore((s) => s.settings)
  const locale = navLocaleFromSettings(settings?.language)

  const [navi, setNavi] = React.useState<NaviBag | null>(null)

  const hydrate = React.useCallback(async () => {
    try {
      const snap = await window.carplay.ipc.readNavigation()
      const patch = unwrapNaviPatch(snap)
      setNavi((prev) => mergeNavi(prev, patch))
    } catch {
      // keep previous state
    }
  }, [])

  React.useEffect(() => {
    void hydrate()

    const handler = (_event: unknown, ...args: unknown[]) => {
      const msg = (args[0] ?? {}) as CarplayEventMsg
      if (msg.type !== 'navigation') return

      const patch = unwrapNaviPatch(msg)
      if (patch) setNavi((prev) => mergeNavi(prev, patch))
      else void hydrate()
    }

    window.carplay.ipc.onEvent(handler)
    return () => window.carplay.ipc.offEvent(handler)
  }, [hydrate])

  const t = React.useMemo(() => translateNavigation(navi, locale), [navi, locale])
  const isActive = navi?.NaviStatus === 1
  const maneuverType = t.codes.ManeuverType
  const turnSide = t.codes.TurnSide
  const remainDistanceText = t.RemainDistanceText
  const etaText = t.TimeRemainingToDestinationText
  const destinationDistanceText = t.DistanceRemainingDisplayStringText
  const clockText = useBlinkingTime()
  const showColon = clockText.includes(':')
  const timeWithColon = clockText.replace(' ', ':')
  const [hh, mm] = timeWithColon.split(':')

  if (!isActive) {
    return (
      <Box
        className={className}
        sx={{
          width: '100%',
          height: '100%',
          display: 'grid',
          placeItems: 'center'
        }}
      >
        <Typography
          sx={{
            fontSize: 30,
            fontWeight: 400,
            lineHeight: 1,
            color: theme.palette.text.primary,
            fontVariantNumeric: 'tabular-nums',
            letterSpacing: 1.9,
            opacity: 0.55
          }}
        >
          {hh}
          <Box
            component="span"
            sx={{
              opacity: showColon ? 1 : 0,
              transition: 'opacity 120ms linear'
            }}
          >
            :
          </Box>
          {mm}
        </Typography>
      </Box>
    )
  }

  return (
    <Box
      className={className}
      sx={{
        width: '100%',
        height: '100%',
        minWidth: 0,
        minHeight: 0,
        display: 'grid',
        gridTemplateRows: 'auto auto auto auto',
        alignItems: 'center',
        justifyItems: 'center',
        rowGap: 0.6
      }}
    >
      {/* ICON */}
      <Box
        sx={{
          width: '100%',
          display: 'grid',
          placeItems: 'center',
          color: theme.palette.text.primary,
          opacity: 1
        }}
      >
        <ManeuverIcon type={maneuverType} turnSide={turnSide} size={iconSize} />
      </Box>

      {/* distance to next maneuver */}
      <Typography
        sx={{
          fontSize: 18,
          fontWeight: 600,
          lineHeight: 1,
          textAlign: 'center',
          color: theme.palette.text.primary,
          whiteSpace: 'nowrap',
          opacity: 1
        }}
      >
        {remainDistanceText ?? '—'}
      </Typography>

      {/* divider line */}
      <Box
        sx={{
          width: '100%',
          height: 0.4,
          borderRadius: 999,
          bgcolor: theme.palette.text.secondary,
          opacity: 0.35
        }}
      />

      {/* bottom row */}
      <Box
        sx={{
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 2.2,
          whiteSpace: 'nowrap'
        }}
      >
        <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.7 }}>
          <AccessTimeIcon sx={{ fontSize: 18, opacity: 0.9 }} />
          <Typography
            sx={{
              fontSize: 16,
              fontWeight: 500,
              lineHeight: 1,
              whiteSpace: 'nowrap',
              fontVariantNumeric: 'tabular-nums'
            }}
          >
            {etaText ?? '—'}
          </Typography>
        </Box>

        <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.7 }}>
          <RouteIcon sx={{ fontSize: 18, opacity: 0.9 }} />
          <Typography
            sx={{
              fontSize: 16,
              fontWeight: 500,
              lineHeight: 1,
              whiteSpace: 'nowrap',
              fontVariantNumeric: 'tabular-nums'
            }}
          >
            {destinationDistanceText ?? '—'}
          </Typography>
        </Box>
      </Box>
    </Box>
  )
}
