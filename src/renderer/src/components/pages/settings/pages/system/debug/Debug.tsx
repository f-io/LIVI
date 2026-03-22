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
import TextField from '@mui/material/TextField'
import Stack from '@mui/material/Stack'

type ProjectionEventMsg = { type: string; payload?: unknown }

function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

type RawSendEncoding = 'utf8' | 'hex' | 'json' | 'base64'

const RAW_MESSAGE_TYPES = [
  { value: 0x08, label: '0x08 Command' },
  { value: 0x10, label: '0x10 DashboardData' },
  { value: 0x29, label: '0x29 GnssData' },
  { value: 0x2a, label: '0x2A MetaData' }
]

function encodeRawPayload(
  value: string,
  encoding: RawSendEncoding,
  appendNul: boolean
): Uint8Array {
  let buf: Uint8Array

  switch (encoding) {
    case 'hex': {
      const clean = value.replace(/\s+/g, '')
      if (clean.length % 2 !== 0) {
        throw new Error('Hex payload length must be even')
      }
      if (!/^[\da-fA-F]*$/.test(clean)) {
        throw new Error('Hex payload contains invalid characters')
      }

      const out = new Uint8Array(clean.length / 2)
      for (let i = 0; i < clean.length; i += 2) {
        out[i / 2] = parseInt(clean.slice(i, i + 2), 16)
      }
      buf = out
      break
    }

    case 'json': {
      const parsed = JSON.parse(value)
      buf = new TextEncoder().encode(JSON.stringify(parsed))
      break
    }

    case 'base64': {
      const decoded = atob(value)
      const out = new Uint8Array(decoded.length)
      for (let i = 0; i < decoded.length; i++) {
        out[i] = decoded.charCodeAt(i)
      }
      buf = out
      break
    }

    case 'utf8':
    default: {
      buf = new TextEncoder().encode(value)
      break
    }
  }

  if (!appendNul) return buf

  const out = new Uint8Array(buf.length + 1)
  out.set(buf, 0)
  out[out.length - 1] = 0
  return out
}

export function Debug() {
  const [navigationSnapshot, setNavigationSnapshot] = React.useState<unknown>(null)
  const [mediaSnapshot, setMediaSnapshot] = React.useState<unknown>(null)

  const [autoUpdateNavSnapshot, setAutoUpdateNavSnapshot] = React.useState(true)
  const [autoUpdateMediaSnapshot, setAutoUpdateMediaSnapshot] = React.useState(true)

  const [events, setEvents] = React.useState<ProjectionEventMsg[]>([])
  const [frozenEvents, setFrozenEvents] = React.useState<ProjectionEventMsg[] | null>(null)

  const [selectedType, setSelectedType] = React.useState<string>('__all__') // DEFAULT: ALL
  const [autoScroll, setAutoScroll] = React.useState(false)
  const [autoUpdateLive, setAutoUpdateLive] = React.useState(true)

  const bottomRef = React.useRef<HTMLDivElement | null>(null)
  const didInitSnapshotsRef = React.useRef(false)

  const autoUpdateNavSnapshotRef = React.useRef(autoUpdateNavSnapshot)
  const autoUpdateMediaSnapshotRef = React.useRef(autoUpdateMediaSnapshot)

  const [rawType, setRawType] = React.useState<number>(0x10)
  const [rawEncoding, setRawEncoding] = React.useState<RawSendEncoding>('json')
  const [rawPayload, setRawPayload] = React.useState('')
  const [rawAppendNul, setRawAppendNul] = React.useState(true)
  const [rawPreview, setRawPreview] = React.useState('')
  const [rawError, setRawError] = React.useState<string | null>(null)

  const eventsRef = React.useRef<ProjectionEventMsg[]>(events)
  React.useEffect(() => {
    eventsRef.current = events
  }, [events])

  React.useEffect(() => {
    autoUpdateNavSnapshotRef.current = autoUpdateNavSnapshot
  }, [autoUpdateNavSnapshot])

  React.useEffect(() => {
    autoUpdateMediaSnapshotRef.current = autoUpdateMediaSnapshot
  }, [autoUpdateMediaSnapshot])

  React.useEffect(() => {
    try {
      const encoded = encodeRawPayload(rawPayload, rawEncoding, rawAppendNul)
      setRawPreview(
        Array.from(encoded)
          .map((b) => b.toString(16).padStart(2, '0'))
          .join('')
      )
      setRawError(null)
    } catch (err) {
      setRawPreview('')
      setRawError(err instanceof Error ? err.message : String(err))
    }
  }, [rawPayload, rawEncoding, rawAppendNul])

  const readNavigationSnapshot = React.useCallback(async () => {
    try {
      const snap = await window.projection.ipc.readNavigation()
      setNavigationSnapshot(snap ?? null)
    } catch {
      setNavigationSnapshot(null)
    }
  }, [])

  const readMediaSnapshot = React.useCallback(async () => {
    try {
      const snap = await window.projection.ipc.readMedia()
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
      const msg = (args[0] ?? {}) as ProjectionEventMsg

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

    window.projection.ipc.onEvent(handler)
    return () => window.projection.ipc.offEvent(handler)
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
      {/* Raw sender */}
      <Accordion defaultExpanded={false}>
        <AccordionSummary expandIcon={<ExpandMoreIcon />}>
          <Typography variant="subtitle2">Raw Projection Message</Typography>
        </AccordionSummary>

        <AccordionDetails>
          <Stack spacing={2}>
            <Stack direction={{ xs: 'column', md: 'row' }} spacing={2}>
              <FormControl size="small" sx={{ minWidth: 220 }}>
                <InputLabel id="raw-message-type-label">Message Type</InputLabel>
                <Select
                  labelId="raw-message-type-label"
                  label="Message Type"
                  value={rawType}
                  onChange={(e) => setRawType(Number(e.target.value))}
                >
                  {RAW_MESSAGE_TYPES.map((item) => (
                    <MenuItem key={item.value} value={item.value}>
                      {item.label}
                    </MenuItem>
                  ))}
                </Select>
              </FormControl>

              <FormControl size="small" sx={{ minWidth: 160 }}>
                <InputLabel id="raw-encoding-label">Encoding</InputLabel>
                <Select
                  labelId="raw-encoding-label"
                  label="Encoding"
                  value={rawEncoding}
                  onChange={(e) => setRawEncoding(e.target.value as RawSendEncoding)}
                >
                  <MenuItem value="utf8">utf8</MenuItem>
                  <MenuItem value="hex">hex</MenuItem>
                  <MenuItem value="json">json</MenuItem>
                  <MenuItem value="base64">base64</MenuItem>
                </Select>
              </FormControl>

              <FormControlLabel
                label="Append NUL"
                control={
                  <Switch
                    checked={rawAppendNul}
                    onChange={(e) => setRawAppendNul(e.target.checked)}
                  />
                }
              />
            </Stack>

            <TextField
              label="Payload"
              multiline
              minRows={6}
              value={rawPayload}
              onChange={(e) => setRawPayload(e.target.value)}
              fullWidth
              spellCheck={false}
              sx={{
                '& .MuiInputBase-input': {
                  fontFamily:
                    'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace'
                }
              }}
            />

            <Paper variant="outlined" sx={{ p: 2 }}>
              <Typography variant="caption" sx={{ display: 'block', mb: 1, opacity: 0.7 }}>
                Encoded payload preview (hex)
              </Typography>

              <pre style={{ margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                {rawError ? `Error: ${rawError}` : rawPreview || '<empty>'}
              </pre>
            </Paper>

            <Box
              sx={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: 1,
                flexWrap: 'wrap'
              }}
            >
              <Button
                size="small"
                variant="outlined"
                onClick={() => {
                  setRawEncoding('hex')
                  setRawPayload(rawPreview)
                  setRawAppendNul(false)
                }}
                disabled={Boolean(rawError) || !rawPreview}
              >
                Use preview as hex input
              </Button>

              <Button
                variant="contained"
                disabled={Boolean(rawError)}
                onClick={() => {
                  try {
                    const encoded = encodeRawPayload(rawPayload, rawEncoding, rawAppendNul)

                    window.projection.ipc.sendRawMessage(rawType, encoded)

                    console.log('[raw-message-sent]', {
                      type: rawType,
                      encoding: rawEncoding,
                      appendNul: rawAppendNul,
                      hex: Array.from(encoded)
                        .map((b) => b.toString(16).padStart(2, '0'))
                        .join('')
                    })
                  } catch (err) {
                    console.error(
                      '[raw-message-send-failed]',
                      err instanceof Error ? err.message : String(err)
                    )
                  }
                }}
              >
                Send
              </Button>
            </Box>
          </Stack>
        </AccordionDetails>
      </Accordion>

      {/* Live */}
      <Accordion defaultExpanded={false}>
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
            <Typography
              variant="subtitle2"
              sx={{
                opacity: 0.8,
                flex: 1,
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis'
              }}
            >
              {visible.length} / {events.length}
            </Typography>

            <FormControlLabel
              label="Scroll"
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
          </Box>
        </AccordionSummary>

        <AccordionDetails>
          <Box
            sx={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              mb: 1,
              gap: 2
            }}
          >
            <FormControl size="small" sx={{ minWidth: 160 }}>
              <InputLabel id="debug-type-label">Type</InputLabel>
              <Select
                labelId="debug-type-label"
                label="Type"
                value={selectedType}
                onChange={(e) => setSelectedType(String(e.target.value))}
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

            <Button
              size="small"
              variant="outlined"
              onClick={() => {
                setEvents([])
                setFrozenEvents(null)
                setSelectedType('__all__')
              }}
            >
              Clear
            </Button>
          </Box>

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
