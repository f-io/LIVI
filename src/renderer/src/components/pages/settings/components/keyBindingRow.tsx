import { useCallback, useEffect, useMemo, useState } from 'react'
import { Box, IconButton, Modal, Paper, Typography } from '@mui/material'
import RestartAltOutlinedIcon from '@mui/icons-material/RestartAltOutlined'
import type { ExtraConfig } from '@main/Globals'
import { DEFAULT_BINDINGS } from '@main/Globals'
import { useCarplayStore } from '@store/store'
import { StackItem } from './stackItem'
import type { KeyBindingNode } from '@renderer/routes/types'
import { useTranslation } from 'react-i18next'

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
  const saveSettings = useCarplayStore((s) => s.saveSettings)
  const settings = useCarplayStore((s) => s.settings) as ExtraConfig | null

  const [capturing, setCapturing] = useState(false)

  const currentValue = useMemo(() => {
    return normalize(settings?.bindings?.[node.bindingKey])
  }, [settings?.bindings, node.bindingKey])

  // default value (from schema node OR DEFAULT_BINDINGS fallback)
  const defaultValue = useMemo(() => {
    const fallback = (DEFAULT_BINDINGS as any)?.[node.bindingKey] as string | undefined
    return normalize(node.defaultValue ?? fallback)
  }, [node.bindingKey, node.defaultValue])

  // IMPORTANT: default can be '' (meaning: unbound)
  const hasDefault = useMemo(() => {
    if (node.defaultValue !== undefined && node.defaultValue !== null) return true
    return Object.prototype.hasOwnProperty.call(DEFAULT_BINDINGS as any, node.bindingKey)
  }, [node.bindingKey, node.defaultValue])

  const isDefault = useMemo(() => {
    if (!hasDefault) return true
    return currentValue === defaultValue
  }, [currentValue, defaultValue, hasDefault])

  const displayValue = capturing ? 'Press a key…' : currentValue || '---'

  const applyValue = useCallback(
    async (code: string) => {
      if (!settings) return

      const next: ExtraConfig = {
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
    if (!hasDefault) return
    await applyValue(defaultValue)
  }, [applyValue, defaultValue, hasDefault])

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

      if (e.key === 'Backspace' || e.key === 'Delete') {
        if (hasDefault) void reset()
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
  }, [applyValue, capturing, hasDefault, reset])

  const label = node.labelKey ? t(node.labelKey) : node.label

  return (
    <>
      <StackItem node={node} onClick={() => setCapturing(true)}>
        <p>{label}</p>

        {/* Right side: value + reset */}
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, whiteSpace: 'nowrap' }}>
          <Box sx={{ fontSize: 'clamp(0.85rem, 2.0svh, 0.95rem)' }}>{displayValue}</Box>

          <IconButton
            size="small"
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
            <Typography variant="h6">Press a key for “{node.label}”</Typography>
            <Typography variant="body2" sx={{ mt: 1 }}>
              Press Esc to cancel.
            </Typography>
            {hasDefault && (
              <Typography variant="body2" sx={{ mt: 0.5 }}>
                Press Backspace/Delete to reset.
              </Typography>
            )}
          </Paper>
        </Box>
      </Modal>
    </>
  )
}
