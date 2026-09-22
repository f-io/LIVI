import VolumeOffRounded from '@mui/icons-material/VolumeOffRounded'
import { Slider, Switch, TextField } from '@mui/material'
import { SettingsNode } from '@renderer/routes'
import type { SelectOption } from '@renderer/routes/types'
import { useLiviStore } from '@renderer/store/store'
import type { Config } from '@shared/types'
import { isHttpUrlInput } from '@shared/utils'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ColorPickerControl } from './colorPicker/ColorPickerControl'
import { extractBtMac, withGhostOption } from './ghostOption'
import NumberSpinner from './numberSpinner/numberSpinner'
import { SelectOptionRow } from './selectOptionRow'
import { getCachedOptions, resolveOptions } from './selectOptionsCache'

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

const SLIDER_SETTLE_MS = 40
const SLIDER_MAX_WAIT_MS = 200

const toPercent = (v: unknown): number => {
  const n = Number(v ?? 1.0)
  return Math.round((Number.isFinite(n) ? n : 1.0) * 100)
}

function VolumeSlider<T>({ value, onChange }: { value: T; onChange: (v: T) => void }) {
  const [draft, setDraft] = useState(() => toPercent(value))
  const dragging = useRef(false)
  const pending = useRef<number | null>(null)
  const settleTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const maxTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    if (!dragging.current) setDraft(toPercent(value))
  }, [value])

  useEffect(() => {
    return () => {
      if (settleTimer.current) clearTimeout(settleTimer.current)
      if (maxTimer.current) clearTimeout(maxTimer.current)
    }
  }, [])

  const flush = () => {
    if (settleTimer.current) {
      clearTimeout(settleTimer.current)
      settleTimer.current = null
    }
    if (maxTimer.current) {
      clearTimeout(maxTimer.current)
      maxTimer.current = null
    }
    const next = pending.current as number
    pending.current = null
    onChange((next / 100) as T)
  }

  return (
    <Slider
      value={draft}
      max={100}
      step={1}
      valueLabelFormat={(v) => (v === 0 ? <VolumeOffRounded /> : `${v}%`)}
      onChange={(_, v) => {
        dragging.current = true
        setDraft(v as number)
        pending.current = v as number
        if (settleTimer.current) clearTimeout(settleTimer.current)
        settleTimer.current = setTimeout(flush, SLIDER_SETTLE_MS)
        if (!maxTimer.current) {
          maxTimer.current = setTimeout(flush, SLIDER_MAX_WAIT_MS)
        }
      }}
      onChangeCommitted={(_, v) => {
        dragging.current = false
        pending.current = v as number
        flush()
      }}
      sx={{
        width: 'calc(100% - 48px)',
        ml: 2,
        mr: 2,
        minWidth: 0
      }}
    />
  )
}

//text field that commits on blur/Enter, NOT per keystroke - a per-keystroke
//onChange writes the whole config each time (saveSettings), which thrashes the
//store and, on the slow kiosk, races the controlled value into duplicated
//characters. Local draft keeps typing smooth; the parent only sees the final
//value. `format: 'url'` blocks the commit until the value is a valid URL.
function StringField({
  value,
  minLength,
  maxLength,
  isUrl,
  onCommit
}: {
  value: string
  minLength?: number
  maxLength?: number
  isUrl: boolean
  onCommit: (v: string) => void
}) {
  const [draft, setDraft] = useState(value)
  const focused = useRef(false)

  //follow external changes only while the user is not editing
  useEffect(() => {
    if (!focused.current) setDraft(value)
  }, [value])

  const tooShort = minLength !== undefined && draft.length < minLength
  const badUrl = isUrl && !isHttpUrlInput(draft)
  const invalid = tooShort || badUrl

  const commit = () => {
    focused.current = false
    if (invalid) return //keep the draft visible with its error, do not write
    const next = isUrl ? draft.trim() : draft
    if (next !== value) onCommit(next)
  }

  return (
    <TextField
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onFocus={() => {
        focused.current = true
      }}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
      }}
      fullWidth
      variant="outlined"
      error={invalid}
      helperText={badUrl ? 'invalid URL' : tooShort ? `min. ${minLength}` : undefined}
      slotProps={{ htmlInput: { maxLength } }}
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

export const SettingsFieldControl = <T,>({
  node,
  value,
  onChange,
  savedLabel,
  onLabelChange,
  onDone
}: Props<T>) => {
  switch (node.type) {
    case 'string':
      return (
        <StringField
          value={String(value ?? '')}
          minLength={node.minLength}
          maxLength={node.maxLength}
          isUrl={node.format === 'url'}
          onCommit={(v) => onChange(v as T)}
        />
      )

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
          // Committed, not per keystroke
          onValueCommitted={(v) => {
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
      return <VolumeSlider value={value} onChange={onChange} />

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
    const sourceOption = (pickedLive ?? pickedOption) as SelectOption
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
    <>
      {renderedOptions.map((o) => (
        <SelectOptionRow
          key={String(o.value)}
          label={labelFor(o)}
          selected={o.value === selectedValue}
          onClick={() => {
            handlePick(o.value)
            onDone?.()
          }}
        />
      ))}
    </>
  )
}
