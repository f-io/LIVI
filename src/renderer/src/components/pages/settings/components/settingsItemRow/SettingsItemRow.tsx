import { Typography } from '@mui/material'
import { StackItem } from '../stackItem'
import { ReactNode } from 'react'

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
