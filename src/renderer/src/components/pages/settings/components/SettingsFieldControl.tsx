import { MenuItem, Select, Slider, Switch, TextField, IconButton } from '@mui/material'
import RestartAltOutlinedIcon from '@mui/icons-material/RestartAltOutlined'
import NumberSpinner from './numberSpinner/numberSpinner'
import { SettingsNode } from '../../../../routes'
import { ExtraConfig } from '@main/Globals'
import { themeColors } from '@renderer/themeColors'

type Props<T> = {
  node: SettingsNode<ExtraConfig>
  value: T
  onChange: (v: T) => void
}

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

export const SettingsFieldControl = <T,>({ node, value, onChange }: Props<T>) => {
  switch (node.type) {
    case 'string':
      return (
        <TextField
          value={(value ?? '') as any}
          onChange={(e) => onChange(e.target.value as T)}
          fullWidth
        />
      )

    case 'number':
      return (
        <NumberSpinner
          size="small"
          value={(value ?? 0) as any}
          onValueChange={(v) => onChange(Number(v) as T)}
        />
      )

    case 'checkbox':
      return <Switch checked={Boolean(value)} onChange={(_, v) => onChange(v as T)} />

    case 'slider':
      return (
        <Slider
          value={Math.round(((value as any) ?? 1.0) * 100)}
          max={100}
          step={5}
          marks
          valueLabelDisplay="auto"
          onChange={(_, v) => onChange(((v as number) / 100) as T)}
          sx={{
            width: 'calc(100% - 48px)',
            ml: 3,
            mr: 3,
            minWidth: 0,
            '& .MuiSlider-valueLabel': { zIndex: 2 }
          }}
        />
      )

    case 'select':
      return (
        <Select
          size="small"
          value={value as any}
          sx={{ minWidth: 200 }}
          onChange={(e) => onChange(e.target.value as T)}
        >
          {node.options.map((o) => (
            <MenuItem key={o.value} value={o.value}>
              {o.label}
            </MenuItem>
          ))}
        </Select>
      )

    case 'color': {
      const hasCustom = value != null && String(value).trim() !== ''
      const color = hasCustom ? (value as unknown as string) : defaultColorForPath(node.path as any)

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
            onClick={() => onChange(undefined as unknown as T)}
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
