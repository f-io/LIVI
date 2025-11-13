import { SettingsLayout } from '../../../../layouts/SettingsLayout'
import { Typography } from '@mui/material'
import Divider from '@mui/material/Divider'
import { StackItem } from '../stackItem'
import { ScreenResolution } from './ScreenResolution'
import { useCallback, useState } from 'react'

export const Video = () => {
  const [isOpen, setIsOpen] = useState(false)

  const handleIsOpen = useCallback(
    (status) => {
      setIsOpen(status)
    },
    [setIsOpen]
  )

  return (
    <SettingsLayout>
      <StackItem onClick={() => handleIsOpen(true)}>
        <Typography>Screen resolution</Typography>
        <Typography>800 x 480</Typography>
      </StackItem>
      <StackItem>
        <Typography>FPS</Typography>
        <Typography>60</Typography>
      </StackItem>
      <StackItem>
        <Typography>Media Delay</Typography>
        <Typography>1000</Typography>
      </StackItem>

      <br />

      <Divider />

      <br />

      <StackItem>
        <Typography>Audio Volume</Typography>
        <Typography>80</Typography>
      </StackItem>
      <StackItem>
        <Typography>Navigation Volume</Typography>
        <Typography>40</Typography>
      </StackItem>

      <ScreenResolution isOpen={isOpen} onClose={handleIsOpen} />
    </SettingsLayout>
  )
}
