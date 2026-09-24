import { Box, Typography } from '@mui/material'
import type { SettingsCustomPageProps } from '@renderer/routes/types'
import { SettingsButtonRow } from '@settings/components'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@shared/assets/carIcons'
import type { Config } from '@shared/types'
import { useLiviStore } from '@store/store'
import React, { useCallback, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { loadImageFromFile, resizeImageToBase64Png } from './utils'

export function IconUploader(_props: SettingsCustomPageProps<Config, unknown>) {
  const { t } = useTranslation()

  const settings = useLiviStore((s) => s.settings)
  const saveSettings = useLiviStore((s) => s.saveSettings)
  const [isImporting, setIsImporting] = useState(false)
  const [isResetting, setIsResetting] = useState(false)

  const fileInputRef = useRef<HTMLInputElement | null>(null)

  const iconPreviewSrc = useMemo(() => {
    const base64 = (
      settings?.carplayIcon180 ||
      settings?.carplayIcon120 ||
      settings?.carplayIcon256 ||
      ICON_180_B64 ||
      ICON_120_B64 ||
      ICON_256_B64
    ).trim()
    if (!base64) return ''
    return `data:image/png;base64,${base64}`
  }, [settings?.carplayIcon120, settings?.carplayIcon180, settings?.carplayIcon256])

  const pickFile = useCallback(() => {
    fileInputRef.current?.click()
  }, [])

  const onFileChange = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      e.target.value = ''
      if (!file) return
      const current = settings as Config

      try {
        setIsImporting(true)

        const img = await loadImageFromFile(file)
        const b120 = resizeImageToBase64Png(img, 120)
        const b180 = resizeImageToBase64Png(img, 180)
        const b256 = resizeImageToBase64Png(img, 256)

        const updated: Config = {
          ...current,
          carplayIcon120: b120,
          carplayIcon180: b180,
          carplayIcon256: b256
        }

        saveSettings(updated)
      } catch (err) {
        console.warn('[IconUploader] import failed', err)
      } finally {
        setIsImporting(false)
      }
    },
    [saveSettings, settings]
  )

  // Empty values are what the CarPlay stack reads as "use the built-in logo".
  const resetToDefaults = useCallback(() => {
    const current = settings as Config
    setIsResetting(true)
    saveSettings({
      ...current,
      carplayIcon120: '',
      carplayIcon180: '',
      carplayIcon256: ''
    })
    setIsResetting(false)
  }, [saveSettings, settings])

  if (!settings) return null

  return (
    <>
      <SettingsButtonRow
        label={t('settings.importPng')}
        buttonLabel={t('settings.import')}
        variant="outlined"
        onClick={pickFile}
        loading={isImporting}
      />

      <SettingsButtonRow
        label={t('settings.uiIcon')}
        buttonLabel={t('settings.reset')}
        variant="outlined"
        onClick={resetToDefaults}
        loading={isResetting}
      />

      <Box sx={{ display: 'flex', justifyContent: 'center', mt: 3 }}>
        <Box
          role="button"
          tabIndex={0}
          aria-label="icon preview"
          onClick={() => !isImporting && pickFile()}
          onKeyDown={(e) => {
            if (!isImporting && (e.key === 'Enter' || e.key === ' ')) {
              e.preventDefault()
              e.stopPropagation()
              pickFile()
            }
          }}
          sx={(theme) => ({
            width: 'clamp(140px, 28svh, 220px)',
            height: 'clamp(140px, 28svh, 220px)',
            borderRadius: 2,
            border: `1px solid ${theme.palette.divider}`,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            overflow: 'hidden',
            cursor: isImporting ? 'default' : 'pointer'
          })}
        >
          {iconPreviewSrc ? (
            <Box
              component="img"
              src={iconPreviewSrc}
              alt="icon preview"
              sx={{ width: '100%', height: '100%', objectFit: 'contain' }}
            />
          ) : (
            <Typography variant="caption" color="text.secondary">
              No icon found
            </Typography>
          )}
        </Box>
      </Box>

      <input
        ref={fileInputRef}
        type="file"
        accept="image/png"
        style={{ display: 'none' }}
        onChange={onFileChange}
      />
    </>
  )
}
