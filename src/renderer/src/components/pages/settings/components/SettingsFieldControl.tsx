import RestartAltOutlinedIcon from '@mui/icons-material/RestartAltOutlined'
import { IconButton, MenuItem, Select, Slider, Switch, TextField } from '@mui/material'
import { themeColors } from '@renderer/themeColors'
import type { Config } from '@shared/types'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingsNode } from '../../../../routes'
import type { SelectOption } from '../../../../routes/types'
import NumberSpinner from './numberSpinner/numberSpinner'
import { getCachedOptions, resolveOptions } from './selectOptionsCache'

type Props<T> = {
  node: SettingsNode<Config>
  value: T
  onChange: (v: T) => void
}

const clampInt = (n: number, min: number, max: number) =>
  Math.min(max, Math.max(min, Math.round(n)))

const defaultColorForPath = (path?: string): string => {
  switch (path) {
    case 'primaryColorDark':
      return themeColors.primaryColorDark
    case 'primaryColorLight':
      return themeColors.primaryColorLight
    case 'highlightColorDark':
      return themeColors.highlightColorDark
    case 'highlightColorLight':
      return themeColors.highlightColorLight
    default:
      return themeColors.highlightColorDark
  }
}

const marks = [
  { value: 0, label: '0%' },
  { value: 25, label: '25%' },
  { value: 50, label: '50%' },
  { value: 75, label: '75%' },
  { value: 100, label: '100%' }
]

export const SettingsFieldControl = <T,>({ node, value, onChange }: Props<T>) => {
  switch (node.type) {
    case 'string':
      return (
        <TextField
          value={String(value ?? '')}
          onChange={(e) => onChange(e.target.value as T)}
          fullWidth
          variant="outlined"
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

            const next = clampInt(v, min, max)
            onChange(next as T)
          }}
        />
      )
    }

    case 'checkbox':
      return <Switch checked={Boolean(value)} onChange={(_, v) => onChange(v as T)} />

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
        />
      )

    case 'color': {
      const hasCustom = value != null && String(value).trim() !== ''
      const color = hasCustom ? String(value) : defaultColorForPath(node.path)

      return (
        <div style={{ height: '100%', display: 'flex', alignItems: 'center', gap: 8 }}>
          <TextField
            type="color"
            value={color}
            onChange={(e) => onChange(e.target.value as T)}
            variant="outlined"
            sx={{
              width: 72,
              minWidth: 72,

              '& .MuiInputBase-root': {
                boxSizing: 'border-box',
                height: 'auto',
                minHeight: 0,
                padding: '0.35em',
                display: 'flex',
                alignItems: 'center'
              },

              '& input[type="color"]': {
                boxSizing: 'border-box',
                width: '100%',
                height: '1.6em',
                padding: 0,
                border: 0,
                cursor: 'pointer'
              }
            }}
          />

          <IconButton
            size="small"
            disabled={!hasCustom}
            onClick={() => onChange(null as unknown as T)}
          >
            <RestartAltOutlinedIcon fontSize="small" />
          </IconButton>
        </div>
      )
    }

    default:
      return null
  }
}

type DynamicSelectProps = {
  node: Extract<SettingsNode<Config>, { type: 'select' }>
  value: string | number
  onChange: (v: unknown) => void
}

function DynamicSelect({ node, value, onChange }: DynamicSelectProps) {
  const { t } = useTranslation()
  const [options, setOptions] = useState<SelectOption[]>(
    () => getCachedOptions(node) ?? node.options
  )

  useEffect(() => {
    if (!node.loadOptions) return
    let alive = true
    void resolveOptions(node).then((opts) => {
      if (alive) setOptions(opts)
    })
    return () => {
      alive = false
    }
  }, [node])

  const inList = options.some((o) => o.value === value)

  return (
    <Select
      size="small"
      variant="outlined"
      value={inList ? value : ''}
      sx={{
        minWidth: 200,
        '&.Mui-focused .MuiOutlinedInput-notchedOutline': {
          borderColor: 'primary.main',
          borderWidth: '1px'
        }
      }}
      onChange={(e) => onChange(e.target.value)}
    >
      {options.map((o) => {
        const label = o.labelKey ? t(o.labelKey, o.label) : o.label
        return (
          <MenuItem key={String(o.value)} value={o.value}>
            {label}
          </MenuItem>
        )
      })}
    </Select>
  )
}
