import * as React from 'react'
import Box from '@mui/material/Box'
import Paper from '@mui/material/Paper'
import Typography from '@mui/material/Typography'
import FormControlLabel from '@mui/material/FormControlLabel'
import Switch from '@mui/material/Switch'
import Select from '@mui/material/Select'
import MenuItem from '@mui/material/MenuItem'
import InputLabel from '@mui/material/InputLabel'
import FormControl from '@mui/material/FormControl'
import Accordion from '@mui/material/Accordion'
import AccordionSummary from '@mui/material/AccordionSummary'
import AccordionDetails from '@mui/material/AccordionDetails'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import Button from '@mui/material/Button'

type CarplayEventMsg = { type: string; payload?: unknown }

function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

export function Debug() {
  const [navigationSnapshot, setNavigationSnapshot] = React.useState<unknown>(null)
  const [mediaSnapshot, setMediaSnapshot] = React.useState<unknown>(null)

  const [autoUpdateNavSnapshot, setAutoUpdateNavSnapshot] = React.useState(true)
  const [autoUpdateMediaSnapshot, setAutoUpdateMediaSnapshot] = React.useState(true)

  const [events, setEvents] = React.useState<CarplayEventMsg[]>([])
  const [frozenEvents, setFrozenEvents] = React.useState<CarplayEventMsg[] | null>(null)

  const [selectedType, setSelectedType] = React.useState<string>('__all__') // DEFAULT: ALL
  const [autoScroll, setAutoScroll] = React.useState(true)
  const [autoUpdateLive, setAutoUpdateLive] = React.useState(true)

  const bottomRef = React.useRef<HTMLDivElement | null>(null)
  const didInitSnapshotsRef = React.useRef(false)

  const autoUpdateNavSnapshotRef = React.useRef(autoUpdateNavSnapshot)
  const autoUpdateMediaSnapshotRef = React.useRef(autoUpdateMediaSnapshot)

  const eventsRef = React.useRef<CarplayEventMsg[]>(events)
  React.useEffect(() => {
    eventsRef.current = events
  }, [events])

  React.useEffect(() => {
    autoUpdateNavSnapshotRef.current = autoUpdateNavSnapshot
  }, [autoUpdateNavSnapshot])

  React.useEffect(() => {
    autoUpdateMediaSnapshotRef.current = autoUpdateMediaSnapshot
  }, [autoUpdateMediaSnapshot])

  const readNavigationSnapshot = React.useCallback(async () => {
    try {
      const snap = await window.carplay.ipc.readNavigation()
      setNavigationSnapshot(snap ?? null)
    } catch {
      setNavigationSnapshot(null)
    }
  }, [])

  const readMediaSnapshot = React.useCallback(async () => {
    try {
      const snap = await window.carplay.ipc.readMedia()
      setMediaSnapshot(snap ?? null)
    } catch {
      setMediaSnapshot(null)
    }
  }, [])

  const readAllSnapshots = React.useCallback(async () => {
    await Promise.all([readNavigationSnapshot(), readMediaSnapshot()])
  }, [readNavigationSnapshot, readMediaSnapshot])

  // IPC listener
  React.useEffect(() => {
    if (!didInitSnapshotsRef.current) {
      didInitSnapshotsRef.current = true
      void readAllSnapshots()
    }

    const handler = (_event: unknown, ...args: unknown[]) => {
      const msg = (args[0] ?? {}) as CarplayEventMsg

      // LIVE (always log)
      setEvents((prev) => {
        const next = [...prev, msg]
        return next.length > 500 ? next.slice(next.length - 500) : next
      })

      // SNAPSHOTS
      if (msg.type === 'navigation' && autoUpdateNavSnapshotRef.current)
        void readNavigationSnapshot()
      if (msg.type === 'media' && autoUpdateMediaSnapshotRef.current) void readMediaSnapshot()
    }

    window.carplay.ipc.onEvent(handler)
    return () => window.carplay.ipc.offEvent(handler)
  }, [readAllSnapshots, readNavigationSnapshot, readMediaSnapshot])

  const sourceEvents = React.useMemo(
    () => (autoUpdateLive ? events : (frozenEvents ?? [])),
    [autoUpdateLive, events, frozenEvents]
  )

  const typeOptions = React.useMemo(() => {
    const set = new Set<string>()
    for (const e of events) {
      if (typeof e?.type === 'string' && e.type.trim()) set.add(e.type)
    }
    return Array.from(set).sort((a, b) => a.localeCompare(b))
  }, [events])

  const visible = React.useMemo(() => {
    const base =
      selectedType === '__all__'
        ? sourceEvents
        : sourceEvents.filter((e) => e.type === selectedType)

    return base.slice(-200)
  }, [sourceEvents, selectedType])

  React.useEffect(() => {
    if (!autoScroll) return
    if (!autoUpdateLive) return
    bottomRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
  }, [visible.length, autoScroll, autoUpdateLive])

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      {/* Live */}
      <Accordion defaultExpanded>
        <AccordionSummary expandIcon={<ExpandMoreIcon />}>
          <Box
            sx={{
              display: 'flex',
              alignItems: 'center',
              width: '100%',
              gap: 2,
              justifyContent: 'space-between'
            }}
          >
            <Typography variant="subtitle2" sx={{ opacity: 0.8, flex: 1 }}>
              Live events ({visible.length} / {events.length})
            </Typography>

            <FormControl size="small" sx={{ minWidth: 220 }}>
              <InputLabel id="debug-type-label">Type</InputLabel>
              <Select
                labelId="debug-type-label"
                label="Type"
                value={selectedType}
                onChange={(e) => setSelectedType(String(e.target.value))}
                onClick={(e) => e.stopPropagation()}
              >
                <MenuItem value="__all__">All</MenuItem>
                <MenuItem value="navigation">navigation</MenuItem>
                <MenuItem value="media">media</MenuItem>
                {typeOptions
                  .filter((t) => t !== 'navigation' && t !== 'media')
                  .map((t) => (
                    <MenuItem key={t} value={t}>
                      {t}
                    </MenuItem>
                  ))}
              </Select>
            </FormControl>

            <FormControlLabel
              label="Auto-scroll"
              control={
                <Switch
                  checked={autoScroll}
                  onChange={(e) => setAutoScroll(e.target.checked)}
                  onClick={(e) => e.stopPropagation()}
                />
              }
              onClick={(e) => e.stopPropagation()}
              onFocus={(e) => e.stopPropagation()}
            />

            <FormControlLabel
              label="Update"
              control={
                <Switch
                  checked={autoUpdateLive}
                  onChange={(e) => {
                    const next = e.target.checked
                    setAutoUpdateLive(next)
                    setFrozenEvents(next ? null : eventsRef.current.slice())
                  }}
                  onClick={(e) => e.stopPropagation()}
                />
              }
              onClick={(e) => e.stopPropagation()}
              onFocus={(e) => e.stopPropagation()}
            />

            <Button
              size="small"
              variant="outlined"
              onClick={(e) => {
                e.stopPropagation()
                setEvents([])
                setFrozenEvents(null)
                setSelectedType('__all__')
              }}
            >
              Clear
            </Button>
          </Box>
        </AccordionSummary>

        <AccordionDetails>
          <Paper
            variant="outlined"
            sx={{
              p: 2,
              minHeight: 320,
              maxHeight: '45vh',
              overflow: 'auto',
              fontFamily:
                'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace'
            }}
          >
            {visible.length ? (
              <>
                {visible.map((m, i) => (
                  <Box
                    key={`${m.type}-${i}`}
                    component="pre"
                    sx={{ m: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}
                  >
                    {safeJson(m)}
                  </Box>
                ))}
                <div ref={bottomRef} />
              </>
            ) : (
              <Typography sx={{ opacity: 0.7 }}>No events yet.</Typography>
            )}
          </Paper>
        </AccordionDetails>
      </Accordion>

      {/* Navigation snapshot */}
      <Accordion>
        <AccordionSummary expandIcon={<ExpandMoreIcon />}>
          <Box
            sx={{
              display: 'flex',
              alignItems: 'center',
              width: '100%',
              gap: 2,
              justifyContent: 'space-between'
            }}
          >
            <Typography variant="subtitle2" sx={{ flex: 1 }}>
              navigationData.json
            </Typography>

            <FormControlLabel
              label="Update"
              control={
                <Switch
                  checked={autoUpdateNavSnapshot}
                  onChange={(e) => {
                    const next = e.target.checked
                    setAutoUpdateNavSnapshot(next)
                    if (next) void readNavigationSnapshot()
                  }}
                  onClick={(e) => e.stopPropagation()}
                />
              }
              onClick={(e) => e.stopPropagation()}
              onFocus={(e) => e.stopPropagation()}
            />
          </Box>
        </AccordionSummary>
        <AccordionDetails>
          <Paper variant="outlined" sx={{ p: 2 }}>
            <pre style={{ margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
              {safeJson(navigationSnapshot)}
            </pre>
          </Paper>
        </AccordionDetails>
      </Accordion>

      {/* Media snapshot */}
      <Accordion>
        <AccordionSummary expandIcon={<ExpandMoreIcon />}>
          <Box
            sx={{
              display: 'flex',
              alignItems: 'center',
              width: '100%',
              gap: 2,
              justifyContent: 'space-between'
            }}
          >
            <Typography variant="subtitle2" sx={{ flex: 1 }}>
              mediaData.json
            </Typography>

            <FormControlLabel
              label="Update"
              control={
                <Switch
                  checked={autoUpdateMediaSnapshot}
                  onChange={(e) => {
                    const next = e.target.checked
                    setAutoUpdateMediaSnapshot(next)
                    if (next) void readMediaSnapshot()
                  }}
                  onClick={(e) => e.stopPropagation()}
                />
              }
              onClick={(e) => e.stopPropagation()}
              onFocus={(e) => e.stopPropagation()}
            />
          </Box>
        </AccordionSummary>
        <AccordionDetails>
          <Paper variant="outlined" sx={{ p: 2 }}>
            <pre style={{ margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
              {safeJson(mediaSnapshot)}
            </pre>
          </Paper>
        </AccordionDetails>
      </Accordion>
    </Box>
  )
}
