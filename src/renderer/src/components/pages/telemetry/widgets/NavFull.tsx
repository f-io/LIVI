import * as React from 'react'
import { Box, Chip, Divider, Stack, Typography } from '@mui/material'

import AccessTimeIcon from '@mui/icons-material/AccessTime'
import RouteIcon from '@mui/icons-material/Route'
import PlaceIcon from '@mui/icons-material/Place'
import AppsIcon from '@mui/icons-material/Apps'
import SignpostIcon from '@mui/icons-material/Signpost'

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
import DirectionsBoatIcon from '@mui/icons-material/DirectionsBoat'
import SwapHorizIcon from '@mui/icons-material/SwapHoriz'
import WrongLocationIcon from '@mui/icons-material/WrongLocation'
import HelpOutlineIcon from '@mui/icons-material/HelpOutline'
import NavigationOutlinedIcon from '@mui/icons-material/NavigationOutlined'

import type { NaviBag } from '../../../../../../main/carplay/services/types'
import { translateNavigation } from '../../../../../../main/carplay/services/utils/translateNavigation'
import type { NavLocale } from '../../../../../../main/carplay/services/utils/translateNavigation'
import { useCarplayStore } from '../../../../store/store'

export type NavFullProps = {
  className?: string
}

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

  // carplay-event message wrapper
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

function RoundaboutIconWithExit({ exitNumber }: { exitNumber: number }) {
  const size = 72
  return (
    <Box
      sx={{
        position: 'relative',
        width: size,
        height: size,
        display: 'grid',
        placeItems: 'center'
      }}
    >
      <RoundaboutRightIcon sx={{ fontSize: size }} />
      <Box sx={{ position: 'absolute', right: -6, bottom: -6 }}>
        <Chip
          size="small"
          label={exitNumber}
          sx={{ height: 20, fontSize: 12, '& .MuiChip-label': { px: 0.8 } }}
        />
      </Box>
    </Box>
  )
}

function ManeuverVisual({
  type,
  turnSide
}: {
  type: number | undefined
  turnSide: number | undefined
}) {
  const size = 72
  const isRight = turnSide === 2

  if (type == null) return <HelpOutlineIcon sx={{ fontSize: size }} />

  if (type >= 28 && type <= 46) {
    return <RoundaboutIconWithExit exitNumber={type - 27} />
  }

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

    case 11:
      return <WrongLocationIcon sx={{ fontSize: size }} />

    case 13:
      return <ForkLeftIcon sx={{ fontSize: size }} />
    case 14:
      return <ForkRightIcon sx={{ fontSize: size }} />

    case 15:
    case 16:
    case 17:
      return <DirectionsBoatIcon sx={{ fontSize: size }} />

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

    case 51:
      return <SwapHorizIcon sx={{ fontSize: size }} />
    case 52:
      return <ForkLeftIcon sx={{ fontSize: size }} />
    case 53:
      return <ForkRightIcon sx={{ fontSize: size }} />

    default:
      return <HelpOutlineIcon sx={{ fontSize: size }} />
  }
}

export function NavFull({ className }: NavFullProps) {
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
      if (patch) {
        setNavi((prev) => mergeNavi(prev, patch))
      } else {
        void hydrate()
      }
    }

    window.carplay.ipc.onEvent(handler)
    return () => window.carplay.ipc.offEvent(handler)
  }, [hydrate])

  const t = React.useMemo(() => translateNavigation(navi, locale), [navi, locale])

  const remainDistanceText =
    isRecord(t) && typeof t.RemainDistanceText === 'string' ? t.RemainDistanceText : undefined

  const isActive = navi?.NaviStatus === 1

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
      <Box
        sx={{
          width: 'min(920px, 100%)',
          display: 'grid',
          placeItems: 'center'
        }}
      >
        {!isActive ? (
          <NavigationOutlinedIcon sx={{ fontSize: 84, opacity: 0.55 }} />
        ) : (
          <Stack spacing={2.2} sx={{ alignItems: 'center', textAlign: 'center' }}>
            <Stack direction="row" spacing={2} alignItems="center" justifyContent="center">
              <Box
                sx={{
                  display: 'flex',
                  flexDirection: 'column',
                  alignItems: 'center',
                  minWidth: 92
                }}
              >
                <ManeuverVisual type={t.codes.ManeuverType} turnSide={t.codes.TurnSide} />

                {remainDistanceText && (
                  <Typography
                    sx={{
                      mt: 0.5,
                      fontSize: 20,
                      fontWeight: 600,
                      letterSpacing: 0.2,
                      lineHeight: 1
                    }}
                  >
                    {remainDistanceText}
                  </Typography>
                )}
              </Box>

              <Box sx={{ minWidth: 0, textAlign: 'left' }}>
                <Typography variant="h5" sx={{ lineHeight: 1.1 }}>
                  {t.ManeuverTypeText}
                </Typography>

                {t.CurrentRoadName && (
                  <Stack direction="row" spacing={1} alignItems="center" sx={{ mt: 0.6 }}>
                    <SignpostIcon fontSize="small" sx={{ opacity: 0.85 }} />
                    <Typography variant="body2" sx={{ opacity: 0.85 }} noWrap>
                      {t.CurrentRoadName}
                    </Typography>
                  </Stack>
                )}
              </Box>
            </Stack>

            <Divider flexItem sx={{ opacity: 0.35 }} />

            <Stack
              direction="row"
              spacing={3}
              flexWrap="wrap"
              justifyContent="center"
              sx={{ rowGap: 1 }}
            >
              {t.TimeRemainingToDestinationText && (
                <Stack direction="row" spacing={1} alignItems="center">
                  <AccessTimeIcon fontSize="small" />
                  <Typography variant="body1">{t.TimeRemainingToDestinationText}</Typography>
                </Stack>
              )}

              {t.DistanceRemainingDisplayStringText && (
                <Stack direction="row" spacing={1} alignItems="center">
                  <RouteIcon fontSize="small" />
                  <Typography variant="body1">{t.DistanceRemainingDisplayStringText}</Typography>
                </Stack>
              )}

              {t.DestinationName && (
                <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
                  <PlaceIcon fontSize="small" />
                  <Typography variant="body1" noWrap sx={{ minWidth: 0 }}>
                    {t.DestinationName}
                  </Typography>
                </Stack>
              )}

              {t.SourceName && (
                <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
                  <AppsIcon fontSize="small" />
                  <Typography variant="body2" sx={{ opacity: 0.85 }} noWrap>
                    {t.SourceName}
                  </Typography>
                </Stack>
              )}
            </Stack>
          </Stack>
        )}
      </Box>
    </Box>
  )
}
