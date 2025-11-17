import { ReactNode } from 'react'

export interface SettingsLayoutProps {
  children?: ReactNode
  onSave: () => boolean
}
