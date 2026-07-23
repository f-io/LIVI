import { Box, Slider, Switch, TextField, Typography } from '@mui/material'
import { useLiviStore } from '@renderer/store/store'
import type { Config } from '@shared/types'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingsNode } from '../../../../routes'
import type { SelectOption } from '../../../../routes/types'
import { ColorPickerControl } from './colorPicker/ColorPickerControl'
import { extractBtMac, withGhostOption } from './ghostOption'
import NumberSpinner from './numberSpinner/numberSpinner'
import { getCachedOptions, resolveOptions } from './selectOptionsCache'
import { StackItem } from './stackItem'

type Props<T> = {
  node: SettingsNode<Config>
  value: T
  onChange: (v: T) => void
  savedLabel?: string
  onLabelChange?: (label: string) => void
  onDone?: () => void
}

const clampInt = (n: number, min: number, max: number, step = 1) => {
  const snapped = step > 1 ? min + Math.round((n - min) / step) * step : Math.round(n)
  return Math.min(max, Math.max(min, snapped))
}

const marks = [
  { value: 0, label: '0%' },
  { value: 25, label: '25%' },
  { value: 50, label: '50%' },
  { value: 75, label: '75%' },
  { value: 100, label: '100%' }
]

export const SettingsFieldControl = <T,>({
  node,
  value,
  onChange,
  savedLabel,
  onLabelChange,
  onDone
}: Props<T>) => {
  switch (node.type) {
    case 'string': {
      const text = String(value ?? '')
      const tooShort = node.minLength !== undefined && text.length < node.minLength
      return (
        <TextField
          value={text}
          onChange={(e) => onChange(e.target.value as T)}
          fullWidth
          variant="outlined"
          error={tooShort}
          helperText={tooShort ? `min. ${node.minLength}` : undefined}
          slotProps={{ htmlInput: { maxLength: node.maxLength } }}
          sx={{
            '& .MuiOutlinedInput-root.Mui-focused .MuiOutlinedInput-notchedOutline': {
              borderColor: 'primary.main',
              borderWidth: '1px'
            },
            '& .MuiInputLabel-root.Mui-focused': {
              color: 'primary.main'
            }
          }}
        />
      )
    }

    case 'number': {
      const min = node.min ?? 0
      const max = node.max ?? Number.MAX_SAFE_INTEGER
      const step = node.step ?? 1

      return (
        <NumberSpinner
          size="medium"
          value={typeof value === 'number' && Number.isFinite(value) ? value : 0}
          min={min}
          max={max}
          step={step}
          onValueChange={(v) => {
            // ignore "in-progress" values
            if (typeof v !== 'number' || !Number.isFinite(v)) return

            const next = clampInt(v, min, max, step)
            onChange(next as T)
          }}
        />
      )
    }

    case 'checkbox':
      return (
        <Switch
          checked={Boolean(value)}
          disabled={node.disabled === true}
          onChange={(_, v) => onChange(v as T)}
        />
      )

    case 'slider':
      return (
        <Slider
          value={Math.round((Number(value ?? 1.0) || 1.0) * 100)}
          max={100}
          step={5}
          marks={marks}
          valueLabelDisplay="off"
          onChange={(_, v) => onChange(((v as number) / 100) as T)}
          sx={{
            width: 'calc(100% - 48px)',
            mt: 1.5,
            ml: 2,
            mr: 2,
            minWidth: 0,
            '& .MuiSlider-valueLabel': { zIndex: 2 }
          }}
        />
      )

    case 'select':
      return (
        <DynamicSelect
          node={node}
          value={value as unknown as string | number}
          onChange={onChange as (v: unknown) => void}
          savedLabel={savedLabel}
          onLabelChange={onLabelChange}
          onDone={onDone}
        />
      )

    case 'color':
      return <ColorPickerControl node={node} value={value} onChange={(v) => onChange(v as T)} />

    default:
      return null
  }
}

type DynamicSelectProps = {
  node: Extract<SettingsNode<Config>, { type: 'select' }>
  value: string | number
  onChange: (v: unknown) => void
  savedLabel?: string
  onLabelChange?: (label: string) => void
  onDone?: () => void
}

function DynamicSelect({
  node,
  value,
  onChange,
  savedLabel,
  onLabelChange,
  onDone
}: DynamicSelectProps) {
  const { t } = useTranslation()
  const audioDevicesRevision = useLiviStore((s) => s.audioDevicesRevision)
  const [options, setOptions] = useState<SelectOption[]>(
    () => getCachedOptions(node) ?? node.options
  )

  useEffect(() => {
    if (!node.loadOptions) return
    let alive = true
    void resolveOptions(node, { force: true }).then((opts) => {
      if (!alive) return
      setOptions(opts)
      // Migrate stored id to live id when MAC matches but profile suffix changed
      const valueMac = extractBtMac(value)
      if (valueMac) {
        const liveMatch = opts.find(
          (o) => !o.offline && extractBtMac(o.value) === valueMac && o.value !== value
        )
        if (liveMatch) onChange(liveMatch.value)
      }
      if (onLabelChange && value !== '' && value !== undefined && value !== null && !savedLabel) {
        const match = opts.find((o) => o.value === value)
        if (match) {
          const live = match.labelKey ? t(match.labelKey, match.label) : match.label
          if (live) onLabelChange(live)
        }
      }
    })
    return () => {
      alive = false
    }
  }, [node, audioDevicesRevision])

  const formatOffline = (name: string): string => t('settings.audioDeviceOffline', { name })
  const renderedOptions = withGhostOption(options, value, savedLabel, formatOffline)
  const inList = renderedOptions.some((o) => o.value === value)

  const handlePick = (next: string | number): void => {
    onChange(next)

    // Offline BT entry → trigger BlueZ Connect
    const pickedOption = renderedOptions.find((o) => o.value === next)
    if (pickedOption?.offline && typeof next === 'string') {
      const mac = extractBtMac(next)
      if (mac) {
        const ipc = window.projection?.ipc
        if (ipc && typeof ipc.connectBluetoothPairedDevice === 'function') {
          void ipc.connectBluetoothPairedDevice(mac).catch(() => {})
        }
      }
    }

    if (!onLabelChange) return
    const pickedLive = options.find((o) => o.value === next)
    const sourceOption = pickedLive ?? pickedOption
    if (!sourceOption) return
    const liveLabel = sourceOption.labelKey
      ? t(sourceOption.labelKey, sourceOption.label)
      : sourceOption.label
    onLabelChange(liveLabel)
  }

  const labelFor = (o: SelectOption): string => {
    const raw = o.labelKey ? t(o.labelKey, o.label) : o.label
    return o.offline ? t('settings.audioDeviceOffline', { name: raw }) : raw
  }

  const selectedValue = inList ? value : ''

  // Flat list instead of a dropdown: one tap selects, selected row gets a dot marker, then close.
  return (
    <Box sx={{ width: '100%' }}>
      {renderedOptions.map((o) => {
        const selected = o.value === selectedValue
        return (
          <StackItem
            key={String(o.value)}
            onClick={() => {
              handlePick(o.value)
              onDone?.()
            }}
          >
            <Typography>{labelFor(o)}</Typography>
            <Box
              sx={{
                flex: 'none',
                width: 'clamp(8px, 1.6svh, 12px)',
                height: 'clamp(8px, 1.6svh, 12px)',
                borderRadius: '50%',
                bgcolor: selected ? 'primary.main' : 'transparent'
              }}
            />
          </StackItem>
        )
      })}
    </Box>
  )
}
