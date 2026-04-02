import { Box, Typography } from '@mui/material'
import type { SettingsNode } from '@renderer/routes/types'
import type { ExtraConfig } from '@shared/types'
import { useLiviStore, useStatusStore } from '@store/store'
import type { Key } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate, useParams } from 'react-router'
import { settingsSchema } from '../../../routes/schemas/schema'
import { SettingsLayout } from '../../layouts'
import { KeyBindingRow, StackItem } from './components'
import { SettingsFieldPage } from './components/SettingsFieldPage'
import { SettingsFieldRow } from './components/SettingsFieldRow'
import { useSmartSettingsFromSchema } from './hooks/useSmartSettingsFromSchema'
import { getNodeByPath, getValueByPath } from './utils'

export function SettingsPage() {
  const navigate = useNavigate()
  const { '*': splat } = useParams()
  const { t } = useTranslation()

  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)

  const path = splat ? splat.split('/') : []
  const node = getNodeByPath(settingsSchema, path)

  const settings = useLiviStore((s) => s.settings) as ExtraConfig

  const { state, handleFieldChange, needsRestart, restart, requestRestart } =
    useSmartSettingsFromSchema(settingsSchema, settings)

  const btDirty = useLiviStore((s) => s.bluetoothPairedDirty)
  const applyBtList = useLiviStore((s) => s.applyBluetoothPairedList)

  const handleRestart = async () => {
    if (!isDongleConnected) return

    if (needsRestart) {
      await restart()
      return
    }

    if (btDirty && typeof applyBtList === 'function') {
      await applyBtList()
    }
  }

  if (!node) return null

  const title = node.labelKey ? t(node.labelKey) : node.label
  const showRestart = Boolean(isDongleConnected) && (Boolean(needsRestart) || Boolean(btDirty))

  if ('path' in node && node.page) {
    return (
      <SettingsLayout title={title} showRestart={showRestart} onRestart={handleRestart}>
        <Box
          sx={{
            flex: 1,
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'flex-start'
          }}
        >
          <SettingsFieldPage
            node={node}
            value={getValueByPath(state, node.path)}
            onChange={(v) => handleFieldChange(node.path, v)}
          />
        </Box>
      </SettingsLayout>
    )
  }

  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
  const children = node.children ?? []

  return (
    <SettingsLayout title={title} showRestart={showRestart} onRestart={handleRestart}>
      {children.map((child: SettingsNode<ExtraConfig>, index: Key | null | undefined) => {
        const _path = child.path as string

        if (child.type === 'route') {
          return (
            <StackItem
              key={index}
              withForwardIcon
              node={child}
              onClick={() => navigate(child.route)}
            >
              <Typography>{child.labelKey ? t(child.labelKey) : child.label}</Typography>
            </StackItem>
          )
        }

        if (child.type === 'custom') {
          return (
            <child.component
              key={child.label}
              state={settings}
              node={child}
              onChange={(v) => handleFieldChange(_path, v)}
              requestRestart={requestRestart}
            />
          )
        }

        if (child.type === 'keybinding') {
          return <KeyBindingRow key={`${_path}:${child.label}`} node={child} />
        }

        return (
          <SettingsFieldRow
            key={_path}
            node={child}
            state={state}
            value={getValueByPath(state, _path)}
            onChange={(v) => handleFieldChange(_path, v)}
            onClick={child.page ? () => navigate(_path) : undefined}
          />
        )
      })}
    </SettingsLayout>
  )
}
