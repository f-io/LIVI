import { Typography } from '@mui/material'
import type { ExtraConfig } from '@shared/types'
import { useTranslation } from 'react-i18next'
import { SettingsNode } from '../../../../routes'
import { getValueByPath } from '../utils'
import { BtDeviceList } from './btDeviceList/BtDeviceList'
import { PosSensitiveList } from './posSensitiveList/PosSensitiveList'
import { SettingsFieldControl } from './SettingsFieldControl'
import { SettingsItemRow } from './settingsItemRow'
import { StackItem } from './stackItem'

type Props<T, K> = {
  node: SettingsNode<ExtraConfig>
  value: T
  state: K
  onChange: (v: T) => void
  onClick?: () => void
}

export const SettingsFieldRow = <T, K>({ node, value, state, onChange, onClick }: Props<T, K>) => {
  const { t } = useTranslation()
  const label = node.labelKey ? t(node.labelKey, node.label) : node.label

  if (node.type === 'posList') {
    return <PosSensitiveList node={node} value={value} onChange={onChange} />
  }

  if (node.type === 'btDeviceList') {
    return <BtDeviceList />
  }

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
