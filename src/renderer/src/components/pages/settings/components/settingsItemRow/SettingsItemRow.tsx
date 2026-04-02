import { Typography } from '@mui/material'
import { ReactNode } from 'react'
import { StackItem } from '../stackItem'

type Props = {
  label: string
  children?: ReactNode
}

export const SettingsItemRow = ({ label, children }: Props) => {
  return (
    <StackItem>
      <Typography>{label}</Typography>
      {children}
    </StackItem>
  )
}
