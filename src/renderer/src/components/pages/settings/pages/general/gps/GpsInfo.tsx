/** Live view of the connected GNSS receiver, fed by the telemetry store. */

import { speedIn, speedSuffix } from '@renderer/utils/units'
import { SettingsValueRow } from '@settings/components'
import type { GnssConstellation, GnssInfo, GnssSatellite, GpsFix } from '@shared/types'
import { DEFAULT_CONFIG } from '@shared/types'
import { useLiviStore } from '@store/store'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useVehicleTelemetry } from '../../../../telemetry/hooks/useVehicleTelemetry'

const CONSTELLATION_LABEL: Record<GnssConstellation, string> = {
  gps: 'GPS',
  glonass: 'GLONASS',
  galileo: 'Galileo',
  beidou: 'BeiDou',
  qzss: 'QZSS',
  unknown: '?'
}

const CONSTELLATION_ORDER: GnssConstellation[] = [
  'gps',
  'glonass',
  'galileo',
  'beidou',
  'qzss',
  'unknown'
]

const FIX_QUALITY_KEY: Record<string, string> = {
  none: 'settings.gpsFixNone',
  gps: 'settings.gpsFixGps',
  dgps: 'settings.gpsFixDgps',
  pps: 'settings.gpsFixPps',
  rtk: 'settings.gpsFixRtk',
  rtkFloat: 'settings.gpsFixRtkFloat',
  estimated: 'settings.gpsFixEstimated',
  manual: 'settings.gpsFixManual',
  simulated: 'settings.gpsFixSimulated'
}

/** Used over visible per constellation. Systems the receiver has ever reported keep
 *  their row at 0 / 0, so the list below does not jump as satellites come and go. */
function constellationRows(
  satellites: GnssSatellite[],
  reported: GnssConstellation[]
): { label: string; value: string }[] {
  const counts = new Map<GnssConstellation, { used: number; visible: number }>()
  for (const sat of satellites) {
    const entry = counts.get(sat.constellation) ?? { used: 0, visible: 0 }
    entry.visible += 1
    if (sat.used) entry.used += 1
    counts.set(sat.constellation, entry)
  }
  const rows: { label: string; value: string }[] = []
  for (const key of CONSTELLATION_ORDER) {
    const entry = counts.get(key)
    if (!entry && !reported.includes(key)) continue
    const { used, visible } = entry ?? { used: 0, visible: 0 }
    rows.push({ label: CONSTELLATION_LABEL[key], value: `${used} / ${visible}` })
  }
  return rows
}

/** C/N0 bands: open sky, usable, slow to acquire, unusable. */
function signalRating(level: number): string {
  if (level >= 44) return 'settings.gpsRatingExcellent'
  if (level >= 38) return 'settings.gpsRatingGood'
  if (level >= 32) return 'settings.gpsRatingWeak'
  return 'settings.gpsRatingPoor'
}

/** Every satellite the receiver hears, so the quality is visible before a fix. */
function heardSnrs(satellites: GnssSatellite[]): number[] {
  return satellites.filter((s) => s.snr !== undefined).map((s) => s.snr as number)
}

/** Fewest satellites a 3D fix needs. */
const FIX_SATELLITES = 4

/**
 * Mean of the four strongest. A mean over all of them would sink as the receiver
 * picks up more low-elevation satellites, which are weak by nature.
 */
function signalLevel(satellites: GnssSatellite[]): number | null {
  const snrs = heardSnrs(satellites).sort((a, b) => b - a)
  if (snrs.length === 0) return null
  const top = snrs.slice(0, FIX_SATELLITES)
  return top.reduce((a, b) => a + b, 0) / top.length
}

function coordinate(fix: GpsFix | undefined): string {
  if (fix?.lat === undefined || fix.lng === undefined) return ''
  return `${fix.lat.toFixed(6)}, ${fix.lng.toFixed(6)}`
}

function utcTime(ms: number): string {
  return `${new Date(ms).toISOString().replace('T', ' ').slice(0, 19)} UTC`
}

type Row = { label: string; value: string; mono?: boolean }

export const GpsInfo = () => {
  const { t } = useTranslation()
  const { telemetry } = useVehicleTelemetry()
  const speedUnit = useLiviStore((s) => s.settings?.speedUnit ?? DEFAULT_CONFIG.speedUnit)

  const gnss = telemetry?.gnss as GnssInfo | undefined
  const fix = telemetry?.gps as GpsFix | undefined

  const rows = useMemo<Row[]>(() => {
    if (!gnss) return []
    const out: Row[] = [
      {
        label: t('settings.gpsStatus'),
        value: gnss.connected
          ? t('settings.gpsConnected')
          : (gnss.error ?? t('settings.gpsDisconnected'))
      },
      ...(gnss.receiverTime !== undefined
        ? [
            {
              label: t('settings.gpsReceiverTime'),
              value: utcTime(gnss.receiverTime),
              mono: true
            }
          ]
        : []),
      ...(gnss.timezone
        ? [{ label: t('settings.gpsTimezone'), value: gnss.timezone, mono: true }]
        : []),
      { label: t('settings.gpsFixQuality'), value: t(FIX_QUALITY_KEY[gnss.fixQuality] ?? '') },
      {
        label: t('settings.gpsFixMode'),
        value: gnss.fixMode === 'none' ? t('settings.gpsFixNone') : gnss.fixMode.toUpperCase()
      },
      {
        label: t('settings.gpsSatellites'),
        value: `${gnss.satellitesUsed} / ${gnss.satellitesVisible}`,
        mono: true
      }
    ]

    for (const row of constellationRows(gnss.satellites, gnss.constellations)) {
      out.push({ label: row.label, value: row.value, mono: true })
    }
    const level = signalLevel(gnss.satellites)
    if (level !== null) {
      out.push({
        label: t('settings.gpsSignal'),
        value: `${level.toFixed(0)} dB-Hz · ${t(signalRating(level))}`,
        mono: true
      })
    }

    // NMEA sends 99.99 as the no-value placeholder
    if (gnss.hdop !== undefined && gnss.hdop < 50) {
      const dop = [gnss.pdop, gnss.hdop, gnss.vdop]
        .map((v) => (v === undefined ? '–' : v.toFixed(1)))
        .join(' / ')
      out.push({
        label: t('settings.gpsDop'),
        value: dop,
        mono: true
      })
    }
    if (fix?.accuracyM !== undefined) {
      out.push({
        label: t('settings.gpsAccuracy'),
        value: `± ${fix.accuracyM.toFixed(1)} m`,
        mono: true
      })
    }

    const position = coordinate(fix)
    if (position) out.push({ label: t('settings.gpsPosition'), value: position, mono: true })
    if (fix?.alt !== undefined) {
      out.push({ label: t('settings.gpsAltitude'), value: `${fix.alt.toFixed(1)} m`, mono: true })
    }
    if (fix?.speedMs !== undefined) {
      out.push({
        label: t('settings.gpsSpeed'),
        value: `${speedIn(fix.speedMs * 3.6, speedUnit).toFixed(1)} ${speedSuffix(speedUnit)}`,
        mono: true
      })
    }

    return out
  }, [gnss, fix, t, speedUnit])

  if (rows.length === 0) {
    return <SettingsValueRow label={t('settings.gpsStatus')} value={t('settings.gpsDisabled')} />
  }

  return (
    <>
      {rows.map((row) => (
        <SettingsValueRow key={row.label} label={row.label} value={row.value} mono={row.mono} />
      ))}
    </>
  )
}
