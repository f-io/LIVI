import { execFileSync, spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'
import { app } from 'electron'
import { resolveHelperBin } from './projection/driver/helper/helperSupervisor'

/** A file from assets/linux, packaged next to the app or read from the repository. */
export function asset(name: string): string {
  const resources = process.resourcesPath
  if (typeof resources === 'string' && resources.length > 0) {
    const packaged = join(resources, name)
    if (existsSync(packaged)) return readFileSync(packaged, 'utf8')
  }
  return readFileSync(join(app.getAppPath(), 'assets', 'linux', name), 'utf8')
}

/** Who a rule is written for: the user behind pkexec or sudo when running under one. */
export function username(): string {
  if (process.env.PKEXEC_UID) {
    try {
      return execFileSync('id', ['-nu', process.env.PKEXEC_UID], { encoding: 'utf8' }).trim()
    } catch {}
  }
  if (process.env.SUDO_USER) return process.env.SUDO_USER
  return os.userInfo().username
}

export function pkexecAvailable(): boolean {
  try {
    execFileSync('which', ['pkexec'], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

/** Whether sudo lists a rule carrying `needle` for this user, or grants everything. */
export function sudoGrants(needle: string): boolean {
  try {
    const out = execFileSync('sudo', ['-n', '-l'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore']
    })
    return out.includes(needle) || /\(ALL(\s*:\s*ALL)?\)\s+NOPASSWD:\s+ALL/.test(out)
  } catch {
    return false
  }
}

/** Runs a shell script as root through pkexec. */
export function runAsRoot(lines: string[]): Promise<void> {
  return new Promise((resolve, reject) => {
    const proc = spawn('pkexec', ['bash', '-c', ['set -e', ...lines].join('\n')], {
      stdio: 'ignore'
    })
    proc.on('close', (code) => {
      if (code === 0) resolve()
      else reject(new Error(`pkexec exited with code ${code}`))
    })
    proc.on('error', reject)
  })
}

/** The lines that put a sudoers file in place. visudo checks it before it lands. */
export function sudoersLines(file: string, content: string): string[] {
  const staged = `${file}.livi-tmp`
  return [
    `trap 'rm -f ${staged}' EXIT`,
    `cat > ${staged} <<'EOF'`,
    content.trimEnd(),
    'EOF',
    `chmod 0440 ${staged}`,
    `chown root:root ${staged}`,
    `visudo -c -f ${staged}`,
    `mv ${staged} ${file}`
  ]
}

/** Short fingerprint of a file's content. */
export function stamp(content: string): string {
  return createHash('sha256').update(content).digest('hex').slice(0, 16)
}

/**
 * Where the app notes what it installed under root, for a file it cannot read back. Holds the
 * stamp of the content, so a changed template means install again.
 */
export function markerPath(name: string): string {
  return join(app.getPath('userData'), name)
}

export function markerHolds(name: string, content: string): boolean {
  try {
    return readFileSync(markerPath(name), 'utf8').trim() === stamp(content)
  } catch {
    return false
  }
}

export function writeMarker(name: string, content: string): void {
  writeFileSync(markerPath(name), stamp(content), { mode: 0o644 })
}

/**
 * Hands rendered files to the helper, which the sudoers rule lets us run as root without a
 * prompt. The files reach the switch in the order given. False when that is not permitted.
 */
export function helperInstalls(what: string, files: Record<string, string>): boolean {
  let dir = ''
  try {
    // resolveHelperBin, not the path: a staged binary older than the app does not know the
    // switch, would ignore it and run as the daemon instead, which never returns.
    const helper = resolveHelperBin()
    dir = mkdtempSync(join(os.tmpdir(), 'livi-install-'))
    const paths = Object.entries(files).map(([name, content]) => {
      const path = join(dir, name)
      writeFileSync(path, content)
      return path
    })
    execFileSync('sudo', ['-n', helper, `--${what}`, ...paths], {
      stdio: 'ignore',
      timeout: 20_000
    })
    return true
  } catch {
    return false
  } finally {
    if (dir) rmSync(dir, { recursive: true, force: true })
  }
}
