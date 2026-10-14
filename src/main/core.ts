import { type ChildProcess, spawn } from 'node:child_process'
import { existsSync, fstatSync } from 'node:fs'
import { join } from 'node:path'
import { logThroughCore } from '@main/logTimestamps'
import type { runtimeStateProps } from '@main/types'
import { uiPlanesPath } from '@main/video'
import { CoreLink, coreSocketPath } from '@shared/core/link'
import { CoreSession } from '@shared/core/session'
import type { Config } from '@shared/types'
import { app } from 'electron'

/** On Linux core starts first, so this only runs out without one. */
const WELCOME_WAIT_MS = 3000
/** On macOS Electron is the app and starts core, unless one answers by then. */
const START_AFTER_MS = 1000
/** Core asks for a restart */
const RELAUNCH_THE_APP = 75
/** "1" when stdin is a pipe from the process that started this one, it ends when that dies. */
const LIFELINE_ENV = 'LIVI_LIFELINE'

let session: CoreSession | null = null
let child: ChildProcess | null = null

function coreProgram(): { bin: string; env: NodeJS.ProcessEnv } | null {
  const bin = app.isPackaged
    ? join(process.resourcesPath, 'core', 'livi-core')
    : join(app.getAppPath(), 'native', 'livi-helperd', 'build', 'Release', 'livi-core')
  if (!existsSync(bin)) return null
  const where = app.isPackaged
    ? { LIVI_RESOURCES: process.resourcesPath }
    : { LIVI_ROOT: app.getAppPath() }
  return {
    bin,
    env: {
      ...process.env,
      ...where,
      LIVI_NO_UI: '1',
      LIVI_VERSION: app.getVersion(),
      [LIFELINE_ENV]: '1',
      LIVI_UI_PLANES: uiPlanesPath()
    }
  }
}

function launchCore(): void {
  const program = coreProgram()
  if (!program) {
    console.warn('[core] no livi-core to start')
    return
  }
  console.log(`[core] starting ${program.bin}`)
  const proc = spawn(program.bin, [], { env: program.env, stdio: ['pipe', 'inherit', 'inherit'] })
  proc.stdin?.on('error', () => logThroughCore(null))
  logThroughCore((line) => proc.stdin?.write(`${line}\n`))
  // Core ends when the user quits LIVI, the app goes with it.
  proc.on('exit', (code) => {
    logThroughCore(null)
    child = null
    console.log(`[core] ended (${code})`)
    if (code === RELAUNCH_THE_APP) app.relaunch()
    app.quit()
  })
  child = proc
}

/** SIGTERM lets core say goodbye to the phones first. */
export function stopCore(): void {
  child?.kill('SIGTERM')
}

/** A dev chain can hand on /dev/null, which would end at once. */
function stdinIsPipe(): boolean {
  try {
    const stat = fstatSync(0)
    return stat.isFIFO() || stat.isSocket()
  } catch {
    return false
  }
}

/** On Linux core starts the UI, which leaves when core dies, even on SIGKILL. */
function leaveWithCore(): void {
  if (process.env[LIFELINE_ENV] !== '1' || !stdinIsPipe()) return
  process.stdin.on('end', () => {
    console.log('[core] core is gone, the UI leaves')
    app.quit()
  })
  process.stdin.resume()
}

/** Follows core's config for what only Electron can do: the windows. */
export function startCore(
  runtimeState: runtimeStateProps,
  onConfig: (next: Config) => void
): Promise<void> {
  leaveWithCore()
  const path = coreSocketPath()
  const current = new CoreSession((msg) => link.send(msg))
  session = current
  let heard = false
  const answered = new Promise<void>((resolve) => {
    const timer = setTimeout(() => {
      console.warn(`[core] no answer from ${path}, the windows start on the defaults`)
      resolve()
    }, WELCOME_WAIT_MS)
    current.subscribe((state) => {
      heard = true
      const next = state.config as Config
      if (next !== runtimeState.config) onConfig(next)
      clearTimeout(timer)
      resolve()
    })
  })
  const link = new CoreLink({
    path,
    client: 'electron',
    onMessage: (msg) => current.receive(msg),
    onClose: () => current.disconnected()
  })
  if (process.platform === 'darwin') {
    setTimeout(() => {
      if (!heard) launchCore()
    }, START_AFTER_MS)
  }
  return answered
}

export function saveConfig(patch: Partial<Config>): void {
  session?.act({ kind: 'setConfig', patch }).catch((e: Error) => {
    console.warn(`[core] config not saved: ${e.message}`)
  })
}
