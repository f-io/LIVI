import { Link, useNavigate } from 'react-router-dom'
import { Typography, Switch, Checkbox } from '@mui/material'
import { StackItem } from '../stackItem'

export const SettingsItemRow = ({ config, item, state, transformer, onClick, onChange }) => {
  const navigate = useNavigate()
  const cfg = config[item]

  const isItemRoute = cfg.type === 'route' && !!cfg.path

  if (isItemRoute) {
    const handleClick = () => {
      navigate(cfg.path)
    }
    return (
      <StackItem value={state[item]} onClick={handleClick} withForwardIcon>
        <Link
          style={{
            pointerEvents: 'none'
          }}
          to={cfg.path as string}
        >
          {cfg.label}
        </Link>
      </StackItem>
    )
  }

  let isClickable = true

  const controlledElement = () => {
    switch (config[item].type) {
      case 'toggle':
        return Switch
      case 'checkbox':
        return Checkbox
      default:
        isClickable = false
        return Typography
    }
  }

  const Element = controlledElement()

  let isChecked = !!state[item]

  if (transformer) {
    isChecked = transformer()
  }

  return (
    <StackItem onClick={onClick}>
      <Typography>{config[item].label}</Typography>

      <Element
        checked={isChecked}
        onChange={(e) => onChange(item, e.target.checked)}
        slotProps={{ input: { 'aria-label': item } }}
        style={{
          justifyContent: 'flex-end',
          pointerEvents: isClickable ? 'auto' : 'none'
        }}
      >
        {state[item]}
      </Element>
    </StackItem>
  )
}
