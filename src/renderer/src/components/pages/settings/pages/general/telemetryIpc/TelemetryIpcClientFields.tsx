/**
 * Renders the IP address + port fields for the "Telemetry (IPC)" setting, but only when
 * `telemetryMode` is `'client'`. The mode itself is a plain `select` node
 * (see generalSchema.ts), which the schema has no way to make sibling rows reactive
 * to — so this block is its own `custom` node, placed directly under the dropdown,
 * that owns that one bit of conditional UI itself.
 */

import { TextField } from '@mui/material'
import type { SettingsCustomPageProps } from '@renderer/routes/types'
import { SettingsItemRow } from '@settings/components'
import type { Config } from '@shared/types'
import { useLiviStore } from '@store/store'
import { useTranslation } from 'react-i18next'

function maskIpv4Input(raw: string): string {
  const cleaned = raw.replace(/[^\d.]/g, '')
  const parts = cleaned.split('.').slice(0, 4)
  return parts
    .map((p) => p.replace(/\D/g, '').slice(0, 3))
    .join('.')
    .slice(0, 15)
}

function isValidIpv4(raw: string): boolean {
  const ip = raw.trim()
  const parts = ip.split('.')
  if (parts.length !== 4) return false
  return parts.every((p) => {
    if (!/^\d+$/.test(p)) return false
    const n = Number(p)
    return n >= 0 && n <= 255
  })
}

function clampPort(raw: string): number {
  const digits = raw.replace(/\D/g, '').slice(0, 5)
  const n = Number(digits)
  if (!digits || Number.isNaN(n)) return 0
  return Math.min(n, 65535)
}

export function TelemetryIpcClientFields(_props: SettingsCustomPageProps<Config, unknown>) {
  const { t } = useTranslation()
  const settings = useLiviStore((s) => s.settings)
  const saveSettings = useLiviStore((s) => s.saveSettings)

  if (settings?.telemetryMode !== 'client') return null

  const ip = settings.telemetryClientIp ?? ''
  const port = settings.telemetryClientPort ?? 0

  const ipError = ip.length > 0 && !isValidIpv4(ip)

  return (
    <>
      <SettingsItemRow label={t('settings.telemetryClientIp', 'IP Address')}>
        <TextField
          size="small"
          placeholder="192.168.1.10"
          sx={{ minWidth: 160 }}
          value={ip}
          onChange={(e) => void saveSettings({ telemetryClientIp: maskIpv4Input(e.target.value) })}
          error={ipError}
          slotProps={{ input: { inputMode: 'numeric', inputProps: { maxLength: 15 } } }}
          helperText={ipError ? t('settings.enterValidIpv4') : ' '}
        />
      </SettingsItemRow>
      <SettingsItemRow label={t('settings.telemetryClientPort', 'Port')}>
        <TextField
          size="small"
          placeholder="4000"
          sx={{ minWidth: 100 }}
          value={port || ''}
          onChange={(e) => void saveSettings({ telemetryClientPort: clampPort(e.target.value) })}
          slotProps={{ input: { inputMode: 'numeric', inputProps: { maxLength: 5 } } }}
          helperText=" "
        />
      </SettingsItemRow>
    </>
  )
}
