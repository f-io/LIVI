import { type ChildProcess, spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { EventEmitter } from 'node:events'
import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync
} from 'node:fs'
import { join } from 'node:path'
import { DEBUG } from '@main/constants'
import type { Config } from '@shared/types'
import { app } from 'electron'
import { loadOrCreateIdentity } from '../cp/stack/identity'

function isInsideAppImageMount(p: string): boolean {
  if (process.env.APPIMAGE && p.startsWith(process.env.APPDIR ?? '')) return true
  return p.includes('/.mount_')
}

function* walkTree(
  root: string,
  prefix = ''
): Generator<{ relPath: string; size: number; mtimeMs: number }> {
  for (const name of readdirSync(root).sort()) {
    const full = join(root, name)
    const rel = prefix ? `${prefix}/${name}` : name
    const st = statSync(full)
    if (st.isDirectory()) {
      yield* walkTree(full, rel)
    } else if (st.isFile()) {
      yield { relPath: rel, size: st.size, mtimeMs: st.mtimeMs }
    }
  }
}

function signTree(root: string): string {
  const h = createHash('sha256')
  for (const e of walkTree(root)) {
    h.update(e.relPath)
    h.update('\0')
    h.update(readFileSync(join(root, e.relPath)))
    h.update('\0')
  }
  return h.digest('hex')
}
function stageHelperDir(sourceRoot: string): string {
  const stageRoot = join(app.getPath('userData'), 'driver', 'unified')
  const sigPath = join(stageRoot, '.livi-staged-sig')
  const wantSig = signTree(sourceRoot)

  if (existsSync(sigPath)) {
    try {
      if (readFileSync(sigPath, 'utf8').trim() === wantSig) return stageRoot
    } catch {
      /* re-stage */
    }
  }

  mkdirSync(stageRoot, { recursive: true })
  cpSync(sourceRoot, stageRoot, { recursive: true, force: true })
  writeFileSync(sigPath, `${wantSig}\n`, { mode: 0o644 })
  if (DEBUG) console.log(`[helper] staged driver/ from AppImage mount to ${stageRoot}`)
  return stageRoot
}
function resolveHelperRoot(): string {
  const entry = join('helper', 'livi-helper.py')
  const devPath = join(__dirname, 'driver')
  if (existsSync(join(devPath, entry))) return devPath

  const resPath =
    typeof process.resourcesPath === 'string' ? join(process.resourcesPath, 'driver') : ''
  if (resPath && existsSync(join(resPath, entry))) {
    if (process.platform === 'linux' && isInsideAppImageMount(resPath)) {
      try {
        return stageHelperDir(resPath)
      } catch (err) {
        if (DEBUG)
          console.warn(
            `[helper] staging failed, falling back to mount path: ${(err as Error).message}`
          )
        return resPath
      }
    }
    return resPath
  }

  return devPath
}
function envFromConfig(cfg: Config): NodeJS.ProcessEnv {
  const wantAaWireless = cfg.wirelessAaEnabled === true
  const wantCpWireless = cfg.wirelessCpEnabled === true
  const identity = loadOrCreateIdentity()

  return {
    ...process.env,
    LIVI_AA_WIRELESS: wantAaWireless ? '1' : '',
    LIVI_CP_WIRELESS: wantCpWireless ? '1' : '',
    DEBUG: DEBUG ? '1' : '',
    LIVI_CP_PK: identity.pkHex,
    LIVI_CP_PI: identity.pairingId,
    LIVI_CP_DEBUG: DEBUG ? '1' : ''
  }
}

export interface HelperSupervisorEvents {
  stdout: (line: string) => void
  stderr: (line: string) => void
  exit: (code: number | null, signal: NodeJS.Signals | null) => void
  error: (err: Error) => void
}

export interface HelperSupervisorOptions {
  python?: string
  restartDelayMs?: number
  maxRestarts?: number
}
export class HelperSupervisor extends EventEmitter {
  private _child: ChildProcess | null = null
  private _stopped = false
  private _restartCount = 0
  private _restartTimer: NodeJS.Timeout | null = null
  private _cfg: Config | null = null
  private readonly _python: string
  private readonly _restartDelayMs: number
  private readonly _maxRestarts: number

  constructor(opts: HelperSupervisorOptions = {}) {
    super()
    this._python = opts.python ?? 'python3'
    this._restartDelayMs = opts.restartDelayMs ?? 2000
    this._maxRestarts = opts.maxRestarts ?? -1
  }

  start(cfg: Config): void {
    this._stopped = false
    this._cfg = cfg
    this._restartCount = 0
    this._spawn()
  }

  async stop(): Promise<void> {
    this._stopped = true
    if (this._restartTimer) {
      clearTimeout(this._restartTimer)
      this._restartTimer = null
    }
    const child = this._child
    if (!child || child.exitCode !== null) return
    await new Promise<void>((resolve) => {
      const onExit = (): void => resolve()
      child.once('exit', onExit)
      child.kill('SIGTERM')
      setTimeout(() => {
        if (child.exitCode === null && !child.killed) child.kill('SIGKILL')
      }, 3000).unref?.()
    })
    this._child = null
  }

  get running(): boolean {
    return this._child !== null && this._child.exitCode === null
  }

  private _spawn(): void {
    if (!this._cfg) return
    const helperRoot = resolveHelperRoot()
    const script = join(helperRoot, 'helper', 'livi-helper.py')

    if (!existsSync(script)) {
      this.emit('error', new Error(`livi-helper.py not found at ${script}`))
      return
    }

    const env = envFromConfig(this._cfg)
    const useSudo = process.platform === 'linux'
    const cmd = useSudo ? 'sudo' : this._python
    const args = useSudo ? ['-n', '-E', this._python, '-u', script] : ['-u', script]

    if (DEBUG) {
      console.log(
        `[helper] spawning ${cmd} ${args.join(' ')} (cwd=${helperRoot}, aa=${env.LIVI_AA_WIRELESS || '0'}, cpWireless=${env.LIVI_CP_WIRELESS || '0'})`
      )
    }

    const child = spawn(cmd, args, {
      cwd: helperRoot,
      env,
      stdio: ['ignore', 'pipe', 'pipe']
    })

    this._child = child
    child.stdout?.setEncoding('utf8')
    child.stderr?.setEncoding('utf8')

    let outBuf = ''
    child.stdout?.on('data', (chunk: string) => {
      outBuf += chunk
      let nl = outBuf.indexOf('\n')
      while (nl !== -1) {
        const line = outBuf.slice(0, nl).replace(/\r$/, '')
        outBuf = outBuf.slice(nl + 1)
        if (line.length > 0) this.emit('stdout', line)
        nl = outBuf.indexOf('\n')
      }
    })

    let errBuf = ''
    child.stderr?.on('data', (chunk: string) => {
      errBuf += chunk
      let nl = errBuf.indexOf('\n')
      while (nl !== -1) {
        const line = errBuf.slice(0, nl).replace(/\r$/, '')
        errBuf = errBuf.slice(nl + 1)
        if (line.length > 0) this.emit('stderr', line)
        nl = errBuf.indexOf('\n')
      }
    })

    child.on('error', (err) => {
      if (DEBUG) console.warn(`[bt] child error: ${err.message}`)
      this.emit('error', err)
    })

    child.on('exit', (code, signal) => {
      if (DEBUG) console.log(`[bt] child exited code=${code} signal=${signal}`)
      this.emit('exit', code, signal)
      this._child = null

      if (this._stopped) return

      this._restartCount += 1
      if (this._maxRestarts >= 0 && this._restartCount > this._maxRestarts) {
        this.emit('error', new Error(`livi-helper.py exceeded max restarts (${this._maxRestarts})`))
        return
      }

      this._restartTimer = setTimeout(() => {
        this._restartTimer = null
        this._spawn()
      }, this._restartDelayMs)
      this._restartTimer.unref?.()
    })
  }
}
