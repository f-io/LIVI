/** Live Wi-Fi link readout under the Wi-Fi settings, fed by the main-process monitor over
 *  settings.onLinkSpeed. Down = phone→car (the stream), up = car→phone; each row shows the live
 *  throughput plus the negotiated PHY rate when the driver reports one. */

import { SettingsValueRow } from '@settings/components'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

type Speed = { downMbps: number; upMbps: number; downRate: number; upRate: number }

const DASH = '—'

/** "12.3 Mbps · 866 PHY", dropping the PHY part when the driver does not report a rate. */
function leg(mbps: number, phy: number): string {
  const rate = `${mbps.toFixed(1)} Mbps`
  return phy > 0 ? `${rate} · ${phy} PHY` : rate
}

export const WifiLinkInfo = () => {
  const { t } = useTranslation()
  const [speed, setSpeed] = useState<Speed | null>(null)

  useEffect(() => window.projection?.settings?.onLinkSpeed?.((_e, s) => setSpeed(s ?? null)), [])

  const down = speed ? leg(speed.downMbps, speed.downRate) : DASH
  const up = speed ? leg(speed.upMbps, speed.upRate) : DASH

  return (
    <>
      <SettingsValueRow label={t('settings.wifiLinkDown')} value={down} mono />
      <SettingsValueRow label={t('settings.wifiLinkUp')} value={up} mono />
    </>
  )
}
