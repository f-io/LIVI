import CloseOutlinedIcon from '@mui/icons-material/CloseOutlined'
import RestartAltOutlinedIcon from '@mui/icons-material/RestartAltOutlined'
import { Box, IconButton, Modal, Paper, Typography } from '@mui/material'
import type { KeyBindingNode } from '@renderer/routes/types'
import type { Config } from '@shared/types'
import { DEFAULT_CONFIG } from '@shared/types'
import { useLiviStore } from '@store/store'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { StackItem } from './stackItem'

const DEFAULT_BINDINGS_MAP = DEFAULT_CONFIG.bindings as Partial<
  Record<KeyBindingNode['bindingKey'], string>
>

function isModifierOnly(code: string) {
  return [
    'ShiftLeft',
    'ShiftRight',
    'ControlLeft',
    'ControlRight',
    'AltLeft',
    'AltRight',
    'MetaLeft',
    'MetaRight'
  ].includes(code)
}

const modalBoxSx = {
  position: 'absolute' as const,
  top: '50%',
  left: '50%',
  transform: 'translate(-50%, -50%)',
  width: 460,
  maxWidth: 'calc(100vw - 48px)'
}

function normalize(v: unknown) {
  return typeof v === 'string' ? v.trim() : ''
}

export function KeyBindingRow({ node }: { node: KeyBindingNode }) {
  const { t } = useTranslation()
  const saveSettings = useLiviStore((s) => s.saveSettings)
  const settings = useLiviStore((s) => s.settings) as Config | null

  const [capturing, setCapturing] = useState(false)
  const [layoutMap, setLayoutMap] = useState<Map<string, string> | null>(null)

  // Resolve physical key codes (event.code) to the active keyboard layout
  useEffect(() => {
    const kb = (
      navigator as unknown as { keyboard?: { getLayoutMap?: () => Promise<Map<string, string>> } }
    ).keyboard
    kb?.getLayoutMap?.()
      .then(setLayoutMap)
      .catch(() => {})
  }, [])

  const currentValue = useMemo(() => {
    return normalize(settings?.bindings?.[node.bindingKey])
  }, [settings?.bindings, node.bindingKey])

  // default value (from schema node OR the default config's bindings)
  const defaultValue = useMemo(() => {
    const fallback = DEFAULT_BINDINGS_MAP[node.bindingKey]
    return normalize(node.defaultValue ?? fallback)
  }, [node.bindingKey, node.defaultValue])

  // IMPORTANT: default can be '' (meaning: unbound)
  const hasDefault = useMemo(() => {
    if (node.defaultValue !== undefined && node.defaultValue !== null) return true
    return Object.prototype.hasOwnProperty.call(DEFAULT_BINDINGS_MAP, node.bindingKey)
  }, [node.bindingKey, node.defaultValue])

  const isDefault = useMemo(() => {
    if (!hasDefault) return true
    return currentValue === defaultValue
  }, [currentValue, defaultValue, hasDefault])

  const keyLabel = useCallback(
    (code: string) => {
      const mapped = layoutMap?.get(code)
      if (mapped) return mapped.length === 1 ? mapped.toUpperCase() : mapped
      return code
    },
    [layoutMap]
  )

  const displayValue = capturing ? 'Press a key…' : currentValue ? keyLabel(currentValue) : '---'

  const applyValue = useCallback(
    async (code: string) => {
      if (!settings) return

      const next: Config = {
        ...settings,
        bindings: {
          ...(settings.bindings ?? {}),
          [node.bindingKey]: code
        }
      }

      await saveSettings(next)
    },
    [node.bindingKey, saveSettings, settings]
  )

  const reset = useCallback(async () => {
    await applyValue(defaultValue)
  }, [applyValue, defaultValue])

  // key capture
  useEffect(() => {
    if (!capturing) return

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()

      if (e.key === 'Escape') {
        setCapturing(false)
        return
      }

      const code = e.code
      if (!code || isModifierOnly(code)) return

      void applyValue(code)
      setCapturing(false)
    }

    document.addEventListener('keydown', onKeyDown, true)
    return () => document.removeEventListener('keydown', onKeyDown, true)
  }, [applyValue, capturing])

  const label = node.labelKey ? t(node.labelKey) : node.label

  return (
    <>
      <StackItem node={node} onClick={() => setCapturing(true)}>
        <p>{label}</p>

        {/* Right side: value + unbind + reset */}
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, whiteSpace: 'nowrap' }}>
          <Box sx={{ fontSize: 'clamp(0.85rem, 2.0svh, 0.95rem)' }}>{displayValue}</Box>

          <IconButton
            size="small"
            tabIndex={-1}
            disabled={!currentValue}
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              setCapturing(false)
              void applyValue('')
            }}
          >
            <CloseOutlinedIcon fontSize="small" />
          </IconButton>

          <IconButton
            size="small"
            tabIndex={-1}
            disabled={!hasDefault || isDefault}
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              setCapturing(false)
              void reset()
            }}
          >
            <RestartAltOutlinedIcon fontSize="small" />
          </IconButton>
        </Box>
      </StackItem>

      <Modal open={capturing} onClose={() => setCapturing(false)}>
        <Box sx={modalBoxSx}>
          <Paper sx={{ p: 2.5 }}>
            <Typography variant="h6">Press a key for &ldquo;{node.label}&rdquo;</Typography>
            <Typography variant="body2" sx={{ mt: 1 }}>
              Press Esc to cancel.
            </Typography>
          </Paper>
        </Box>
      </Modal>
    </>
  )
}
