import { Typography } from '@mui/material'
import { StackItem } from './stackItem'
import { SettingsItemRow } from './settingsItemRow'
import { SettingsFieldControl } from './SettingsFieldControl'
import { SettingsNode } from '../../../../routes'
import { getValueByPath } from '../utils'
import { ExtraConfig } from '../../../../../../main/Globals'
import { useTranslation } from 'react-i18next'

type Props<T, K> = {
  node: SettingsNode<ExtraConfig>
  value: T
  state: K
  onChange: (v: T) => void
  onClick?: () => void
}

export const SettingsFieldRow = <T, K>({ node, value, state, onChange, onClick }: Props<T, K>) => {
  const { t } = useTranslation()
  const label = node.labelKey ? t(node.labelKey) : node.label

  if (onClick) {
    return (
      <StackItem
        withForwardIcon
        onClick={onClick}
        node={node}
        value={getValueByPath(state, node.path)}
        showValue={node.displayValue}
      >
        <Typography>{label}</Typography>
      </StackItem>
    )
  }

  return (
    <SettingsItemRow label={label}>
      <SettingsFieldControl node={node} value={value} onChange={onChange} />
    </SettingsItemRow>
  )
}
