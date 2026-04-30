/**
 * aaBluetoothSupervisor — runs the Python BT/Wi-Fi stack
 * (`bt/aa-bluetooth.py`) as a child process and keeps it alive for the lifetime
 * of an AA session.
 *
 * The Python process owns:
 *   - BlueZ RFCOMM agent + AA Phone Profile registration
 *   - Wi-Fi AP bring-up (hostapd) and DHCP (dnsmasq)
 *   - Wi-Fi Info handover that flips the phone from BT-control onto the AP
 *
 * We never modify the Python source. Configuration flows in via ENV variables
 * read by `bt/config.py` (Step 6) — keeps the boundary clean and makes ad-hoc
 * Linux-side debugging (just run the .py with the same env) trivial.
 */

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
import type { DongleConfig } from '@shared/types'
import { app } from 'electron'

/**
 * AppImage mounts the squashfs payload at `/tmp/.mount_LIVI-XXXXXX/...` via
 * libfuse with mode 0700 owned by the invoking user. When the supervisor
 * re-execs via `sudo`, root cannot read that mount at all (FUSE enforces the
 * mode regardless of DAC bypass) — the python interpreter then dies with
 * `[Errno 13] Permission denied`.
 *
 * Same problem balenaEtcher hit and solved by staging `etcher-util` into
 * `app.getPath('userData')` before invoking elevation. We follow the same
 * pattern: copy `bt/` out of the mount once per app version into userData
 * (root has DAC bypass on regular filesystems) and point the supervisor at
 * the staged path.
 */
function isInsideAppImageMount(p: string): boolean {
  // $APPIMAGE / $APPDIR are set by AppRun. The `.mount_` substring covers
  // edge cases where someone re-execs without the wrapper.
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

/** Cheap content fingerprint (path + size + mtime) — re-stages on app upgrade. */
function signTree(root: string): string {
  const h = createHash('sha256')
  for (const e of walkTree(root)) {
    h.update(e.relPath)
    h.update('\0')
    h.update(String(e.size))
    h.update('\0')
    h.update(String(Math.floor(e.mtimeMs)))
    h.update('\0')
  }
  return h.digest('hex')
}

/** Copy `bt/` out of the AppImage mount into userData, return staged path. */
function stageBtDir(sourceRoot: string): string {
  const stageRoot = join(app.getPath('userData'), 'aa', 'bt')
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
  // dereference: false because the tree is plain .py files; force: true so
  // a partial previous stage doesn't block re-copy.
  cpSync(sourceRoot, stageRoot, { recursive: true, force: true })
  writeFileSync(sigPath, `${wantSig}\n`, { mode: 0o644 })
  console.log(`[aaBT] staged bt/ from AppImage mount to ${stageRoot}`)
  return stageRoot
}

/** Where the bt/ folder lives at runtime: dev tree vs. packaged extraResources. */
function resolveBtRoot(): string {
  // Dev: src/main/services/projection/driver/aa/aaBluetoothSupervisor.ts → ./bt
  const devPath = join(__dirname, 'bt')
  if (existsSync(join(devPath, 'aa-bluetooth.py'))) return devPath

  // Packaged: electron-builder.yml extraResources lays bt/ next to resources.
  const resPath =
    typeof process.resourcesPath === 'string' ? join(process.resourcesPath, 'aa', 'bt') : ''
  if (resPath && existsSync(join(resPath, 'aa-bluetooth.py'))) {
    if (process.platform === 'linux' && isInsideAppImageMount(resPath)) {
      try {
        return stageBtDir(resPath)
      } catch (err) {
        console.warn(`[aaBT] staging failed, falling back to mount path: ${(err as Error).message}`)
        return resPath
      }
    }
    return resPath
  }

  // Last-ditch: parent of __dirname (covers built `out/main/...` next to source).
  return devPath
}

/** Map a flat LIVI DongleConfig into the ENV variables aa-bluetooth.py reads. */
function envFromConfig(cfg: DongleConfig): NodeJS.ProcessEnv {
  // wifiType '2.4ghz' → channel 6 default; '5ghz' → 36 default. Honour
  // explicit cfg.wifiChannel when set.
  const channel =
    Number.isFinite(cfg.wifiChannel) && cfg.wifiChannel > 0
      ? String(cfg.wifiChannel)
      : cfg.wifiType === '5ghz'
        ? '36'
        : '6'

  // SSID + BT name both follow `carName`.
  const name = cfg.carName?.trim() ? cfg.carName : 'LIVI'

  return {
    ...process.env,
    LIVI_SSID: name,
    LIVI_BTNAME: name,
    LIVI_PASSPHRASE: cfg.wifiPassword || '12345678',
    LIVI_CHANNEL: channel,
    LIVI_COUNTRY: 'DE',
    LIVI_PORT: '5277',
    LIVI_WIFI_IFACE: cfg.wifiInterface || 'wlan0',
    LIVI_BT_ADAPTER: cfg.btAdapter || 'hci0'
  }
}

export interface AaBluetoothSupervisorEvents {
  /** Underlying python process emitted a line on stdout. */
  stdout: (line: string) => void
  /** Underlying python process emitted a line on stderr. */
  stderr: (line: string) => void
  /** Process exited (cleanly or via signal). */
  exit: (code: number | null, signal: NodeJS.Signals | null) => void
  /** Spawn or runtime error from the child. */
  error: (err: Error) => void
}

export interface AaBluetoothSupervisorOptions {
  /** Override path to the python interpreter (default: 'python3'). */
  python?: string
  /** Backoff between restarts in ms (default: 2000). */
  restartDelayMs?: number
  /** Cap consecutive restarts; -1 = unlimited (default: -1). */
  maxRestarts?: number
}

/**
 * Owns a single python3 child running aa-bluetooth.py.
 *
 * Lifecycle:
 *   start(cfg) → spawn → on exit (unless stopped) → backoff → respawn
 *   stop()    → no more restarts, SIGTERM (then SIGKILL after grace period)
 */
export class AaBluetoothSupervisor extends EventEmitter {
  private _child: ChildProcess | null = null
  private _stopped = false
  private _restartCount = 0
  private _restartTimer: NodeJS.Timeout | null = null
  private _cfg: DongleConfig | null = null
  private readonly _python: string
  private readonly _restartDelayMs: number
  private readonly _maxRestarts: number

  constructor(opts: AaBluetoothSupervisorOptions = {}) {
    super()
    this._python = opts.python ?? 'python3'
    this._restartDelayMs = opts.restartDelayMs ?? 2000
    this._maxRestarts = opts.maxRestarts ?? -1
  }

  /** Bring the supervisor up. Returns immediately; spawn happens async. */
  start(cfg: DongleConfig): void {
    this._stopped = false
    this._cfg = cfg
    this._restartCount = 0
    this._spawn()
  }

  /** Tear it down. Idempotent. */
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
      // Grace period — escalate if the process is still alive.
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
    const btRoot = resolveBtRoot()
    const script = join(btRoot, 'aa-bluetooth.py')

    if (!existsSync(script)) {
      this.emit('error', new Error(`aa-bluetooth.py not found at ${script}`))
      return
    }

    const env = envFromConfig(this._cfg)

    // aa-bluetooth.py refuses to run unless euid==0 (it needs CAP_NET_ADMIN,
    // BlueZ root D-Bus, hostapd, dnsmasq). On Linux we re-exec via
    // `sudo -n -E python3 …`. The matching sudoers drop-in is installed once
    // at first run by aaSudoers.checkAndInstallAaSudoers (mirrors the udev
    // rule installer). `-n` makes sudo refuse instead of prompting if the rule
    // is missing — the supervisor's exit-loop then surfaces that in the log.
    // `-E` preserves the LIVI_* env vars; the sudoers SETENV: tag whitelists
    // them. Off-Linux (dev/macOS) we fall back to plain python3.
    //
    // `-u` forces unbuffered stdout/stderr on the python interpreter. Without
    // it, aa-bluetooth.py's print() calls (which mostly omit flush=True) sit
    // in libc buffers and only land in our log when python flushes — which
    // means a crash before the first explicit flush appears in the log as
    // "no python output at all", masking how far it actually got. Cheap and
    // safe; the python source stays untouched.
    const useSudo = process.platform === 'linux'
    const cmd = useSudo ? 'sudo' : this._python
    const args = useSudo ? ['-n', '-E', this._python, '-u', script] : ['-u', script]

    console.log(
      `[aaBT] spawning ${cmd} ${args.join(' ')} (cwd=${btRoot}, ssid=${env.LIVI_SSID}, btname=${env.LIVI_BTNAME}, ch=${env.LIVI_CHANNEL})`
    )

    const child = spawn(cmd, args, {
      cwd: btRoot,
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
      console.warn(`[aaBT] child error: ${err.message}`)
      this.emit('error', err)
    })

    child.on('exit', (code, signal) => {
      console.log(`[aaBT] child exited code=${code} signal=${signal}`)
      this.emit('exit', code, signal)
      this._child = null

      if (this._stopped) return

      this._restartCount += 1
      if (this._maxRestarts >= 0 && this._restartCount > this._maxRestarts) {
        this.emit(
          'error',
          new Error(`aa-bluetooth.py exceeded max restarts (${this._maxRestarts})`)
        )
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
