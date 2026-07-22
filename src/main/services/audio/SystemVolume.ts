import { DEBUG } from '@main/constants'
import { type ChildProcess, execFile, spawn } from 'child_process'

const PACTL = 'pactl'
const DEFAULT_SINK = '@DEFAULT_SINK@'
const CALL_TIMEOUT_MS = 2_000
const RESTART_DELAY_MS = 2_000
const ECHO_WINDOW_MS = 400
const READ_DEBOUNCE_MS = 120

let lastWriteAt = 0
/** The single running sink watcher, so restarts never stack another pactl process. */
let monitor: { stop: () => void } | null = null

function sinkName(configuredDevice: string | undefined): string {
  const s = configuredDevice?.trim()
  return s ? s : DEFAULT_SINK
}

function run(args: string[]): Promise<string | null> {
  if (process.platform !== 'linux') return Promise.resolve(null)
  return new Promise((resolve) => {
    execFile(PACTL, args, { timeout: CALL_TIMEOUT_MS }, (err, stdout) => {
      if (err) {
        if (DEBUG) console.warn(`[SystemVolume] pactl ${args.join(' ')} failed: ${err.message}`)
        resolve(null)
        return
      }
      resolve(stdout)
    })
  })
}

export function parseSinkVolume(stdout: string): number | null {
  const m = stdout.match(/(\d+)\s*%/)
  if (!m) return null
  const pct = Number(m[1])
  if (!Number.isFinite(pct)) return null
  return Math.min(1, Math.max(0, pct / 100))
}

/** Current level of the configured sink, 0.0 to 1.0, or null if it cannot be read. */
export async function getSystemVolume(configuredDevice?: string): Promise<number | null> {
  const out = await run(['get-sink-volume', sinkName(configuredDevice)])
  return out === null ? null : parseSinkVolume(out)
}

/** Set the configured sink to the given level, 0.0 to 1.0. */
export async function setSystemVolume(level: number, configuredDevice?: string): Promise<boolean> {
  const clamped = Math.min(1, Math.max(0, level))
  const pct = Math.round(clamped * 100)
  lastWriteAt = Date.now()
  const sink = sinkName(configuredDevice)
  const out = await run(['set-sink-volume', sink, `${pct}%`])
  if (out === null) {
    console.warn(`[SystemVolume] could not set ${sink} to ${pct} %, is pactl installed?`)
    return false
  }
  lastWriteAt = Date.now()
  console.log(`[SystemVolume] system volume → ${pct} %`)
  return true
}

/**
 * Watch the sink for level changes made outside LIVI and report them as 0.0 to 1.0.
 * Changes we caused ourselves are suppressed for a short window so the two sides
 * cannot chase each other.
 */
export function startSystemVolumeMonitor(
  configuredDevice: () => string | undefined,
  onChange: (level: number) => void
): void {
  if (monitor) return
  if (process.platform !== 'linux') {
    console.log('[SystemVolume] not available on this platform, link stays inactive')
    return
  }

  let stopped = false
  let child: ChildProcess | null = null
  let restartTimer: ReturnType<typeof setTimeout> | null = null
  let readTimer: ReturnType<typeof setTimeout> | null = null
  let last: number | null = null

  const readSoon = (): void => {
    if (readTimer) clearTimeout(readTimer)
    readTimer = setTimeout(async () => {
      readTimer = null
      if (stopped) return
      if (Date.now() - lastWriteAt < ECHO_WINDOW_MS) return
      const level = await getSystemVolume(configuredDevice())
      if (level === null || stopped) return
      if (last !== null && Math.abs(level - last) < 0.005) return
      last = level
      if (DEBUG) console.log(`[SystemVolume] sink reports ${Math.round(level * 100)} %`)
      onChange(level)
    }, READ_DEBOUNCE_MS)
  }

  const spawnMonitor = (): void => {
    if (stopped) return
    const proc = spawn(PACTL, ['subscribe'], { stdio: ['ignore', 'pipe', 'ignore'] })
    child = proc

    let buf = ''
    proc.stdout?.setEncoding('utf8')
    proc.stdout?.on('data', (chunk: string) => {
      buf += chunk
      let nl: number
      while ((nl = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, nl)
        buf = buf.slice(nl + 1)
        if (/Event 'change' on sink #/.test(line)) readSoon()
      }
    })
    proc.on('error', (err) => {
      if (DEBUG) console.warn(`[SystemVolume] subscribe failed: ${err.message}`)
    })
    proc.on('exit', () => {
      child = null
      if (stopped) return
      restartTimer = setTimeout(spawnMonitor, RESTART_DELAY_MS)
    })
  }

  spawnMonitor()
  console.log('[SystemVolume] watching the sink for changes made outside LIVI')

  monitor = {
    stop: () => {
      stopped = true
      if (restartTimer) clearTimeout(restartTimer)
      if (readTimer) clearTimeout(readTimer)
      if (child && !child.killed) {
        try {
          child.kill()
        } catch {
          /* already gone */
        }
      }
    }
  }
}

/** Stop the sink watcher and kill its pactl process. */
export function stopSystemVolumeMonitor(): void {
  if (!monitor) return
  monitor.stop()
  monitor = null
  console.log('[SystemVolume] sink watcher stopped')
}
