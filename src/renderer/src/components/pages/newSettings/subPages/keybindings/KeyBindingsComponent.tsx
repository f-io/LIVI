import { ExtraConfig } from '@main/Globals'
import { useEffect, useState, useCallback } from 'react'
import Grid from '@mui/material/Grid'
import { Box, Button, Modal, Paper, styled, Typography } from '@mui/material'

interface KeyBindingsProps {
  settings: ExtraConfig
  updateKey: (key: 'bindings', value: ExtraConfig['bindings']) => void
}

const Item = styled(Paper)(({ theme }) => ({
  backgroundColor: theme.palette.mode === 'dark' ? '#1A2027' : '#fff',
  ...theme.typography.body2,
  padding: theme.spacing(1),
  textAlign: 'center',
  color: theme.palette.text.secondary
}))

const style = {
  position: 'absolute' as const,
  top: '50%',
  left: '50%',
  transform: 'translate(-50%, -50%)',
  width: 400,
  bgcolor: 'background.paper',
  boxShadow: 24,
  p: 4,
  justifyContent: 'center'
}

export function KeyBindings({ settings, updateKey }: KeyBindingsProps) {
  const [keyToBind, setKeyToBind] = useState<string>('')
  const [openWaiting, setOpenWaiting] = useState<boolean>(false)

  const setKey = useCallback(
    (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()

      if (e.key === 'Escape') {
        setOpenWaiting(false)
        setKeyToBind('')
        return
      }

      const bindings = { ...settings.bindings }
      bindings[keyToBind] = e.code
      updateKey('bindings', bindings)
      setOpenWaiting(false)
      setKeyToBind('')
    },
    [keyToBind, settings.bindings, updateKey]
  )

  useEffect(() => {
    if (!openWaiting) return
    const handler = (e: KeyboardEvent) => setKey(e)
    document.addEventListener('keydown', handler, true)
    return () => {
      document.removeEventListener('keydown', handler, true)
    }
  }, [openWaiting, setKey])

  const awaitKeyPress = (keyName: string) => {
    setKeyToBind(keyName)
    setOpenWaiting(true)
  }

  return (
    <>
      <Grid container spacing={2}>
        {Object.entries(settings?.bindings ?? {}).map(([action, code]) => (
          <Grid size={{ xs: 3 }} key={action}>
            <Item>
              <Typography variant="subtitle2">{action}</Typography>
              <Button
                variant="outlined"
                onClick={() => awaitKeyPress(action)}
                disabled={openWaiting}
              >
                {code as React.ReactNode}
              </Button>
            </Item>
          </Grid>
        ))}
      </Grid>

      <Modal
        open={openWaiting}
        onClose={() => setOpenWaiting(false)}
        aria-labelledby="await-key-bind-title"
        aria-describedby="await-key-bind-description"
      >
        <Box sx={style}>
          <Typography id="await-key-bind-title" variant="h6">
            Press key for “{keyToBind}”
          </Typography>
          <Typography id="await-key-bind-description" variant="body2" sx={{ mt: 1 }}>
            Press Esc to cancel.
          </Typography>
        </Box>
      </Modal>
    </>
  )
}
