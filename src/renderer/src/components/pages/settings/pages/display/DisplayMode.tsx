import { Menu, MenuItem, Typography } from '@mui/material'
import type { Config } from '@shared/types'
import { useLiviStore } from '@store/store'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { StackItem } from '../../components/stackItem/StackItem'

const PANEL_DEFAULT = ''

export function DisplayMode({ state }: { state: Config }) {
  const { t } = useTranslation()
  const [modes, setModes] = useState<string[]>([])
  const [open, setOpen] = useState(false)
  const anchor = useRef<HTMLDivElement>(null)
  const saveSettings = useLiviStore((s) => s.saveSettings)

  useEffect(() => {
    let alive = true
    window.app
      ?.listDisplayModes?.()
      .then((m) => {
        if (alive) setModes(Array.isArray(m) ? m : [])
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])

  if (!state.kiosk?.main || modes.length === 0) return null

  const pick = (mode: string): void => {
    setOpen(false)
    saveSettings({ ...state, displayMode: mode })
  }

  const current = modes.includes(state.displayMode) ? state.displayMode : PANEL_DEFAULT

  return (
    <div ref={anchor}>
      <StackItem
        withForwardIcon
        showValue
        value={current || t('settings.displayModeDefault')}
        onClick={() => setOpen(true)}
      >
        <Typography>{t('settings.displayMode')}</Typography>
      </StackItem>
      <Menu anchorEl={anchor.current} open={open} onClose={() => setOpen(false)}>
        <MenuItem selected={current === PANEL_DEFAULT} onClick={() => pick(PANEL_DEFAULT)}>
          {t('settings.displayModeDefault')}
        </MenuItem>
        {modes.map((m) => (
          <MenuItem key={m} selected={m === current} onClick={() => pick(m)}>
            {m}
          </MenuItem>
        ))}
      </Menu>
    </div>
  )
}
