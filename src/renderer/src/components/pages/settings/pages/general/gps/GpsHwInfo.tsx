/** Module identity and RF front end — the hardware side, apart from the live data. */

import { SettingsValueRow } from '@settings/components'
import type { GnssAntennaPower, GnssAntennaStatus, GnssInfo, GnssJamming } from '@shared/types'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useVehicleTelemetry } from '../../../../telemetry/hooks/useVehicleTelemetry'

const ANTENNA_STATUS_KEY: Record<GnssAntennaStatus, string> = {
  init: 'settings.gpsAntennaInit',
  unknown: 'settings.gpsAntennaUnknown',
  ok: 'settings.gpsAntennaOk',
  short: 'settings.gpsAntennaShort',
  open: 'settings.gpsAntennaOpen'
}

const ANTENNA_POWER_KEY: Record<GnssAntennaPower, string> = {
  off: 'settings.gpsAntennaPowerOff',
  on: 'settings.gpsAntennaPowerOn',
  unknown: 'settings.gpsAntennaUnknown'
}

const JAMMING_KEY: Record<GnssJamming, string> = {
  unknown: 'settings.gpsAntennaUnknown',
  ok: 'settings.gpsJammingOk',
  warning: 'settings.gpsJammingWarning',
  critical: 'settings.gpsJammingCritical'
}

type Row = { label: string; value: string; mono?: boolean }

export const GpsHwInfo = () => {
  const { t } = useTranslation()
  const { telemetry } = useVehicleTelemetry()
  const gnss = telemetry?.gnss as GnssInfo | undefined

  const rows = useMemo<Row[]>(() => {
    const out: Row[] = []
    const version = gnss?.version
    if (version?.model) out.push({ label: t('settings.gpsModel'), value: version.model })
    if (version?.firmware) {
      out.push({ label: t('settings.gpsFirmware'), value: version.firmware, mono: true })
    }
    if (version?.protocol) {
      out.push({ label: t('settings.gpsProtocol'), value: version.protocol, mono: true })
    }
    if (version?.software) {
      out.push({ label: t('settings.gpsSoftware'), value: version.software, mono: true })
    }
    if (version?.hardware) {
      out.push({ label: t('settings.gpsHardware'), value: version.hardware, mono: true })
    }
    if (version?.supported?.length) {
      out.push({ label: t('settings.gpsSupported'), value: version.supported.join(', ') })
    }

    const rf = gnss?.rf
    if (rf) {
      out.push({
        label: t('settings.gpsAntennaStatus'),
        value: t(ANTENNA_STATUS_KEY[rf.antennaStatus])
      })
      out.push({
        label: t('settings.gpsAntennaPower'),
        value: t(ANTENNA_POWER_KEY[rf.antennaPower])
      })
      out.push({
        label: t('settings.gpsJamming'),
        value: `${t(JAMMING_KEY[rf.jamming])} · ${rf.jammingIndicator}`,
        mono: true
      })
      out.push({ label: t('settings.gpsAgc'), value: String(rf.agc), mono: true })
      out.push({ label: t('settings.gpsNoise'), value: String(rf.noise), mono: true })
    }
    return out
  }, [gnss, t])

  if (rows.length === 0) {
    return <SettingsValueRow label={t('settings.gpsStatus')} value={t('settings.gpsNoHwInfo')} />
  }

  return (
    <>
      {rows.map((row) => (
        <SettingsValueRow key={row.label} label={row.label} value={row.value} mono={row.mono} />
      ))}
    </>
  )
}
