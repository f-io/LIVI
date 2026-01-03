import React, { ReactNode } from 'react'

export interface SettingsLayoutProps {
  children?: ReactNode
  title: string
  showRestart: boolean
  onRestart?: () => void | Promise<void>
}

export interface AppLayoutProps {
  navRef: React.RefObject<HTMLDivElement | null>
  receivingVideo: boolean
}
