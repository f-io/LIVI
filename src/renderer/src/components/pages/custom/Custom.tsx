import WidgetsOutlinedIcon from '@mui/icons-material/WidgetsOutlined'
import { useTheme } from '@mui/material/styles'
import Typography from '@mui/material/Typography'
import { useLiviStore } from '@store/store'
import { useEffect, useState } from 'react'

export function Custom() {
  const theme = useTheme()
  const [url, setUrl] = useState<string | null | undefined>(undefined)
  const [loadedKey, setLoadedKey] = useState<string | null>(null)
  const customUrl = useLiviStore((s) => s.settings?.customUrl)
  const dark = useLiviStore((s) => s.settings?.darkMode !== false)

  useEffect(() => {
    let cancelled = false
    window.app
      ?.customPageUrl?.()
      .then((next) => !cancelled && setUrl(next))
      .catch(() => !cancelled && setUrl(null))
    return () => {
      cancelled = true
    }
  }, [customUrl])

  const frameKey = `${url}-${dark ? 'dark' : 'light'}`

  return (
    <div
      id="custom-root"
      style={{
        position: 'absolute',
        inset: 0,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: '1rem',
        backgroundColor: theme.palette.background.default
      }}
    >
      {url ? (
        <iframe
          key={frameKey}
          title="custom"
          src={url}
          onLoad={() => setLoadedKey(frameKey)}
          style={{
            position: 'absolute',
            inset: 0,
            width: '100%',
            height: '100%',
            border: 0,
            backgroundColor: theme.palette.background.default,
            visibility: loadedKey === frameKey ? 'visible' : 'hidden'
          }}
        />
      ) : (
        <>
          <WidgetsOutlinedIcon sx={{ fontSize: 96, color: 'text.disabled' }} />
          {url === null && (
            <Typography variant="body2" color="text.disabled">
              No page in the custom folder
            </Typography>
          )}
        </>
      )}
    </div>
  )
}
