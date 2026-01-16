import React, { useCallback, useMemo, useRef, useState } from 'react'
import { Box, Button, CircularProgress, Stack, Typography } from '@mui/material'
import type { ExtraConfig } from '@main/Globals'
import type { SettingsCustomPageProps } from '@renderer/routes/types'
import { useCarplayStore, useStatusStore } from '@store/store'
import { loadImageFromFile, resizeImageToBase64Png } from './utils'
import { ResetDongleIconsResult } from './types'

export function IconUploader(props: SettingsCustomPageProps<ExtraConfig, unknown>) {
  const { requestRestart } = props

  const settings = useCarplayStore((s) => s.settings)
  const saveSettings = useCarplayStore((s) => s.saveSettings)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)

  const [isImporting, setIsImporting] = useState(false)
  const [isUploading, setIsUploading] = useState(false)
  const [isResetting, setIsResetting] = useState(false)
  const [message, setMessage] = useState<string>('')

  const fileInputRef = useRef<HTMLInputElement | null>(null)

  const iconPreviewSrc = useMemo(() => {
    const base64 =
      settings?.dongleIcon180 || settings?.dongleIcon120 || settings?.dongleIcon256 || ''
    if (!base64.trim()) return ''
    return `data:image/png;base64,${base64.trim()}`
  }, [settings?.dongleIcon120, settings?.dongleIcon180, settings?.dongleIcon256])

  const pickFile = useCallback(() => {
    fileInputRef.current?.click()
  }, [])

  const onFileChange = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      e.target.value = ''
      if (!file) return
      if (!settings) return

      try {
        setIsImporting(true)
        setMessage('')

        const img = await loadImageFromFile(file)
        const b120 = resizeImageToBase64Png(img, 120)
        const b180 = resizeImageToBase64Png(img, 180)
        const b256 = resizeImageToBase64Png(img, 256)

        const updated: ExtraConfig = {
          ...settings,
          dongleIcon120: b120,
          dongleIcon180: b180,
          dongleIcon256: b256
        }

        saveSettings(updated)
        setMessage('Icon imported. You can upload it to your dongle now.')
      } catch (err) {
        console.warn('[IconUploader] import failed', err)
        setMessage('Icon import failed.')
      } finally {
        setIsImporting(false)
      }
    },
    [saveSettings, settings]
  )

  const uploadToDongle = useCallback(async () => {
    try {
      setIsUploading(true)
      setMessage('')

      await window.carplay.usb.uploadIcons()

      requestRestart?.()

      setMessage('Icon upload done.')
    } catch (err) {
      console.warn('[IconUploader] upload failed', err)
      setMessage('Icon upload failed.')
    } finally {
      setIsUploading(false)
    }
  }, [requestRestart])

  const resetToDefaults = useCallback(async () => {
    if (!settings) return

    try {
      setIsResetting(true)
      setMessage('')

      const api = (window as any).app
      const fn = api?.resetDongleIcons as undefined | (() => Promise<ResetDongleIconsResult>)
      if (typeof fn !== 'function') {
        setMessage('Reset API not available.')
        return
      }

      const result = await fn()
      const updated: ExtraConfig = {
        ...settings,
        dongleIcon120: result.dongleIcon120 ?? settings.dongleIcon120,
        dongleIcon180: result.dongleIcon180 ?? settings.dongleIcon180,
        dongleIcon256: result.dongleIcon256 ?? settings.dongleIcon256
      }

      saveSettings(updated)
      setMessage('Icons reset to defaults.')
    } catch (err) {
      console.warn('[IconUploader] reset failed', err)
      setMessage('Resetting icons failed.')
    } finally {
      setIsResetting(false)
    }
  }, [saveSettings, settings])

  if (!settings) return null

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Box
        role="button"
        tabIndex={0}
        onClick={() => !isImporting && pickFile()}
        onKeyDown={(e) => {
          if (!isImporting && (e.key === 'Enter' || e.key === ' ')) {
            e.preventDefault()
            e.stopPropagation()
            pickFile()
          }
        }}
        sx={(theme) => ({
          width: 160,
          height: 160,
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

      <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
        <Button variant="outlined" onClick={pickFile} disabled={isImporting}>
          {isImporting ? 'Importing…' : 'Import PNG'}
        </Button>

        <Button variant="outlined" onClick={resetToDefaults} disabled={isResetting}>
          {isResetting ? 'Resetting…' : 'Reset'}
        </Button>

        <Button
          variant="contained"
          onClick={uploadToDongle}
          disabled={isUploading || !isDongleConnected}
        >
          {isUploading ? 'Uploading…' : 'Upload'}
        </Button>

        {(isImporting || isUploading || isResetting) && <CircularProgress size={18} />}
      </Stack>

      {message && (
        <Typography variant="body2" color="text.secondary">
          {message}
        </Typography>
      )}

      <input
        ref={fileInputRef}
        type="file"
        accept="image/png"
        style={{ display: 'none' }}
        onChange={onFileChange}
      />
    </Box>
  )
}
