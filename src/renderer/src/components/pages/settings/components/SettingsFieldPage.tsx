import { Typography } from '@mui/material'
import { SettingsFieldControl } from './SettingsFieldControl'
import { SettingsNode } from '../../../../routes'
import { ExtraConfig } from '@main/Globals'

type Props<T> = {
  node: SettingsNode<ExtraConfig>
  value: T
  onChange: (v: T) => void
}

export const SettingsFieldPage = <T,>({ node, value, onChange }: Props<T>) => {
  return (
    <>
      <SettingsFieldControl node={node} value={value} onChange={onChange} />

      {node.page?.description && (
        <Typography color="text.secondary" mb={2}>
          {node.page.description}
        </Typography>
      )}
    </>
  )
}
