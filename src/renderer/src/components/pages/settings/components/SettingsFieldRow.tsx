import { Typography } from '@mui/material'
import { StackItem } from './stackItem'
import { SettingsItemRow } from './settingsItemRow'
import { SettingsFieldControl } from './SettingsFieldControl'
import { SettingsNode } from '../../../../routes'
import { getValueByPath } from '../utils'
import { ExtraConfig } from '../../../../../../main/Globals'

type Props<T, K> = {
  node: SettingsNode<ExtraConfig>
  value: T
  state: K
  onChange: (v: T) => void
  onClick?: () => void
}

export const SettingsFieldRow = <T, K>({ node, value, state, onChange, onClick }: Props<T, K>) => {
  if (onClick) {
    return (
      <StackItem
        withForwardIcon
        onClick={onClick}
        node={node}
        value={getValueByPath(state, node.path)}
        showValue={node.displayValue}
      >
        <Typography>{node.label}</Typography>
      </StackItem>
    )
  }

  return (
    <SettingsItemRow label={node.label}>
      <SettingsFieldControl node={node} value={value} onChange={onChange} />
    </SettingsItemRow>
  )
}
