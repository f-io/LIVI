import { spawn, ChildProcessWithoutNullStreams, execSync } from 'child_process'
import { EventEmitter } from 'events'
import os from 'os'
import fs from 'fs'

export default class NodeMicrophone extends EventEmitter {
  private process: ChildProcessWithoutNullStreams | null = null
  private readonly device: string
  private readonly rate: number = 16000
  private readonly channels: number = 1
  private readonly format: string = 'S16_LE'

  constructor() {
    super()
    this.device = NodeMicrophone.resolveSysdefaultDevice()
    console.debug('[NodeMicrophone] Using device:', this.device)
  }

  start(): void {
    this.stop()

    let cmd = ''
    let args: string[] = []
    const env = { ...process.env, PATH: NodeMicrophone.buildExecPath(process.env.PATH) }

    if (os.platform() === 'linux') {
      cmd = 'arecord'
      args = [
        '-D', this.device,
        '-f', this.format,
        '-c', this.channels.toString(),
        '-r', this.rate.toString(),
        '-t', 'raw',
        '-q',
        '-',
      ]
    } else if (os.platform() === 'darwin') {
      const recPath = NodeMicrophone.resolveRecPath()
      if (!recPath) {
        console.error('[NodeMicrophone] SoX (rec) not found. Install with: brew install sox')
        return
      }
      cmd = recPath
      args = [
        '-b', '16',
        '-c', this.channels.toString(),
        '-r', this.rate.toString(),
        '-e', 'signed-integer',
        '-t', 'raw',
        '-q',
        '-',
      ]
    } else {
      console.error('[NodeMicrophone] Platform not supported for microphone recording')
      return
    }

    console.debug('[NodeMicrophone] PATH =', env.PATH)
    console.debug(`[NodeMicrophone] Spawning ${cmd} with args:`, args.join(' '))

    this.process = spawn(cmd, args, { env, shell: false })

    const proc = this.process
    if (!proc) {
      console.error('[NodeMicrophone] Failed to spawn recorder process')
      this.cleanup()
      return
    }

    proc.stdout.on('data', (chunk: Buffer) => this.emit('data', chunk))
    proc.stderr.on('data', (d: Buffer) => {
      const s = d.toString().trim()
      if (s) console.warn('[NodeMicrophone] STDERR:', s)
    })
    proc.on('error', (err) => {
      console.error('[NodeMicrophone] Error:', err)
      this.cleanup()
    })
    proc.on('close', (code) => {
      console.debug('[NodeMicrophone] recorder exited with code', code)
      this.cleanup()
    })

    console.debug('[NodeMicrophone] Recording started')
  }

  stop(): void {
    if (this.process) {
      console.debug('[NodeMicrophone] Stopping recording')
      try { this.process.kill() } catch (e) {
        console.warn('[NodeMicrophone] Failed to kill process:', e)
      }
      this.cleanup()
    } else {
      console.debug('[NodeMicrophone] No active process to stop')
    }
  }

  private cleanup(): void {
    this.process = null
  }

  // macOS: find SoX/rec
  private static resolveRecPath(): string | null {
    const fromEnv = process.env.SOX_REC_PATH
    if (fromEnv && fs.existsSync(fromEnv)) return fromEnv

    const candidates = [
      '/opt/homebrew/bin/rec',   // Apple Silicon
      '/usr/local/bin/rec',      // Intel
    ]
    for (const p of candidates) if (fs.existsSync(p)) return p

    try {
      const widened = NodeMicrophone.buildExecPath(process.env.PATH)
      const out = execSync('which rec', { encoding: 'utf8', env: { ...process.env, PATH: widened } }).trim()
      if (out && fs.existsSync(out)) return out
    } catch { }

    return null
  }

  private static buildExecPath(current?: string): string {
    const extra = [
      '/opt/homebrew/bin',
      '/usr/local/bin',
      '/usr/bin',
      '/bin',
      '/usr/sbin',
      '/sbin',
    ]
    const set = new Set<string>([...extra, ...(current ? current.split(':') : [])])
    return Array.from(set).join(':')
  }

  static resolveSysdefaultDevice(): string {
    if (os.platform() === 'linux') {
      try {
        const output = execSync('arecord -L', { encoding: 'utf8' })
        const lines = output.split('\n')
        for (const line of lines) {
          const m = line.trim().match(/^sysdefault:CARD=([^\s,]+)/)
          if (m) return `plughw:CARD=${m[1]},DEV=0`
        }
        console.warn('[NodeMicrophone] sysdefault card not found, falling back')
        return 'plughw:0,0'
      } catch (e) {
        console.warn('[NodeMicrophone] Failed to resolve sysdefault device', e)
        return 'plughw:0,0'
      }
    } else if (os.platform() === 'darwin') {
      return 'default'
    } else {
      return 'unsupported'
    }
  }

  static getSysdefaultPrettyName(): string {
    if (os.platform() === 'linux') {
      try {
        const result = execSync('arecord -L', { encoding: 'utf8' })
        const lines = result.split('\n')
        const idx = lines.findIndex(l => l.trim().startsWith('sysdefault:'))
        if (idx === -1) return 'not available'
        const desc = lines[idx + 1]?.trim()
        return desc && desc !== 'sysdefault' ? desc : 'not available'
      } catch (e) {
        console.warn('[NodeMicrophone] Failed to get sysdefault mic label', e)
        return 'not available'
      }
    } else if (os.platform() === 'darwin') {
      return 'system default'
    } else {
      return 'not available'
    }
  }
}
