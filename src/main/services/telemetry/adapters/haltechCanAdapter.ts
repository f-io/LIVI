import { type ChildProcess, spawn } from 'node:child_process'
import type { TelemetryStore } from '../TelemetryStore'
import { decodeHaltechCanLine } from './haltechCanDecoder'

const CAPTURE_HELPER = '/usr/libexec/livi-can-capture'
const CAN_INTERFACE = 'can0'
const RETRY_DELAY_MS = 2000
const STALE_AFTER_MS = 1500

export type HaltechCanAdapterDeps = {
  store: TelemetryStore
  enabled: boolean
  spawnCapture?: () => ChildProcess
}

export type HaltechCanAdapterHandle = {
  off: () => void
  setEnabled: (enabled: boolean) => void
}

function spawnCapture(): ChildProcess {
  if (process.env.LIVI_OS === '1') {
    return spawn('/usr/bin/sudo', ['-n', CAPTURE_HELPER], {
      stdio: ['ignore', 'pipe', 'pipe']
    })
  }
  return spawn('candump', ['-L', `${CAN_INTERFACE},360:7FF,370:7FF`], {
    stdio: ['ignore', 'pipe', 'pipe']
  })
}

export function attachHaltechCanAdapter({
  store,
  enabled: initiallyEnabled,
  spawnCapture: createCapture = spawnCapture
}: HaltechCanAdapterDeps): HaltechCanAdapterHandle {
  let enabled = false
  let child: ChildProcess | null = null
  let outputBuffer = ''
  let retryTimer: NodeJS.Timeout | null = null
  let staleTimer: NodeJS.Timeout | null = null
  let lastRpmAt: number | null = null
  let lastSpeedAt: number | null = null

  const clearStaleValues = (): void => {
    if (lastRpmAt !== null) {
      lastRpmAt = null
      store.merge({ rpm: 0 })
    }
    if (lastSpeedAt !== null) {
      lastSpeedAt = null
      store.merge({ speedKph: 0 })
    }
  }

  const stop = (): void => {
    if (retryTimer) clearTimeout(retryTimer)
    retryTimer = null
    if (staleTimer) clearInterval(staleTimer)
    staleTimer = null
    const running = child
    child = null
    outputBuffer = ''
    running?.kill('SIGTERM')
    clearStaleValues()
  }

  const scheduleRetry = (): void => {
    if (!enabled || retryTimer) return
    retryTimer = setTimeout(() => {
      retryTimer = null
      start()
    }, RETRY_DELAY_MS)
  }

  const consumeLine = (line: string): void => {
    const patch = decodeHaltechCanLine(line)
    if (!patch) return
    if (typeof patch.rpm === 'number') lastRpmAt = Date.now()
    if (typeof patch.speedKph === 'number') lastSpeedAt = Date.now()
    store.merge(patch)
  }

  const start = (): void => {
    if (!enabled || child) return
    try {
      const process = createCapture()
      child = process
      outputBuffer = ''

      process.stdout?.setEncoding('utf8')
      process.stdout?.on('data', (chunk: string) => {
        outputBuffer += chunk
        let newline = outputBuffer.indexOf('\n')
        while (newline >= 0) {
          consumeLine(outputBuffer.slice(0, newline))
          outputBuffer = outputBuffer.slice(newline + 1)
          newline = outputBuffer.indexOf('\n')
        }
        if (outputBuffer.length > 4096) {
          console.warn('[haltech-can] discarded oversized capture line')
          outputBuffer = ''
        }
      })

      process.stderr?.setEncoding('utf8')
      process.stderr?.on('data', (chunk: string) => {
        const detail = chunk.trim()
        if (detail) console.warn(`[haltech-can] capture: ${detail}`)
      })

      process.once('error', (error) => {
        console.warn('[haltech-can] capture process failed:', error)
      })
      process.once('exit', (code, signal) => {
        if (child !== process) return
        child = null
        outputBuffer = ''
        if (code !== 0 && signal !== 'SIGTERM') {
          console.warn(`[haltech-can] capture exited (code=${code}, signal=${signal})`)
        }
        scheduleRetry()
      })

      if (!staleTimer) {
        staleTimer = setInterval(() => {
          const now = Date.now()
          if (lastRpmAt !== null && now - lastRpmAt > STALE_AFTER_MS) {
            lastRpmAt = null
            store.merge({ rpm: 0 })
          }
          if (lastSpeedAt !== null && now - lastSpeedAt > STALE_AFTER_MS) {
            lastSpeedAt = null
            store.merge({ speedKph: 0 })
          }
        }, 500)
      }
    } catch (error) {
      console.warn('[haltech-can] could not start capture:', error)
      scheduleRetry()
    }
  }

  const setEnabled = (nextEnabled: boolean): void => {
    if (enabled === nextEnabled) return
    enabled = nextEnabled
    if (enabled) start()
    else stop()
  }

  setEnabled(initiallyEnabled)

  return {
    setEnabled,
    off: (): void => {
      enabled = false
      stop()
    }
  }
}
