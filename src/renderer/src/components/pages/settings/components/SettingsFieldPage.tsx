import { Typography } from '@mui/material'
import type { ExtraConfig } from '@shared/types'
import { useTranslation } from 'react-i18next'
import { SettingsNode } from '../../../../routes'
import { SettingsFieldControl } from './SettingsFieldControl'

type Props<T> = {
  node: SettingsNode<ExtraConfig>
  value: T
  onChange: (v: T) => void
}

export const SettingsFieldPage = <T,>({ node, value, onChange }: Props<T>) => {
  const { t } = useTranslation()
  const description = node.page?.labelDescription
    ? t(node.page?.labelDescription)
    : node.page?.description
  return (
    <>
      <SettingsFieldControl node={node} value={value} onChange={onChange} />

      {description && (
        <Typography color="text.secondary" sx={{ mb: 2 }}>
          {description}
        </Typography>
      )}
    </>
  )
}
