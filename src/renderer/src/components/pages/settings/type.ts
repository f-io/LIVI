import type { Config } from '@shared/types'
import { ReactNode } from 'react'
import { SettingsNode } from '../../../routes'

export interface StackItemProps {
  children?: ReactNode
  withForwardIcon?: boolean
  value?: unknown
  showValue?: boolean
  onClick?: () => void
  node?: SettingsNode<Config>
}

export type SettingsCustomPageProps<TState = Config, TValue = unknown> = {
  state: TState
  onChange: (value: TValue) => void
}
