import { execFile, execFileSync, spawn } from 'node:child_process'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { app, type BrowserWindow, dialog } from 'electron'

const execFileAsync = promisify(execFile)

export type PackageEntry = {
  section: 'core' | 'lite'
  name: string
  probe: string
  purpose: string
  /** Fedora/dnf package name. */
  fedora: string
}

/** Manifest ships next to the app in production and lives in scripts/install during dev. */
function manifestPath(): string | null {
  const candidates = [
    join(process.resourcesPath ?? '', 'packages.txt'),
    join(app.getAppPath(), 'scripts', 'install', 'packages.txt'),
    join(process.cwd(), 'scripts', 'install', 'packages.txt')
  ]
  return candidates.find((p) => existsSync(p)) ?? null
}

export function parseManifest(text: string): PackageEntry[] {
  const out: PackageEntry[] = []
  for (const raw of text.split('\n')) {
    const line = raw.trim()
    if (!line || line.startsWith('#')) continue
    const [section, name, probe, purpose, fedora] = line.split('|')
    if ((section !== 'core' && section !== 'lite') || !name || !probe) continue
    out.push({
      section,
      name: name.trim(),
      probe: probe.trim(),
      purpose: (purpose ?? '').trim(),
      fedora: (fedora ?? '').trim()
    })
  }
  return out
}

export function readManifest(): PackageEntry[] {
  const path = manifestPath()
  if (!path) return []
  try {
    return parseManifest(readFileSync(path, 'utf8'))
  } catch {
    return []
  }
}

/** True when a desktop session is absent, which is what the lite packages backfill.
 * A display server in the environment counts as a session: labwc and friends do
 * not export XDG_CURRENT_DESKTOP, but any session hands us its display. */
function isLiteHost(): boolean {
  if (process.env.WAYLAND_DISPLAY || process.env.DISPLAY) return false
  return !process.env.XDG_CURRENT_DESKTOP && !existsSync('/usr/bin/gnome-session')
}

/** Packages the manifest requires on this host, core plus lite where there is no desktop. */
export function requiredPackages(entries = readManifest()): PackageEntry[] {
  const lite = isLiteHost()
  return entries.filter((e) => e.section === 'core' || (lite && e.section === 'lite'))
}

/** Existence test for a path holding at most one `*`, which stands in for a single
 * directory name. Libraries sit under a multiarch directory that differs per host. */
export function pathPresent(pattern: string): boolean {
  const star = pattern.indexOf('*')
  if (star < 0) return existsSync(pattern)
  const base = pattern.slice(0, pattern.lastIndexOf('/', star))
  const slash = pattern.indexOf('/', star)
  const rest = slash < 0 ? '' : pattern.slice(slash + 1)
  // Fedora keeps libraries flat in /usr/lib64.
  if (existsSync(join(`${base}64`, rest))) return true
  try {
    return readdirSync(base).some((entry) => existsSync(join(base, entry, rest)))
  } catch {
    return false
  }
}

/** Run one manifest probe. Unknown probe kinds count as present, so a typo never nags. */
async function probeSatisfied(probe: string): Promise<boolean> {
  const [kind, arg] = [probe.slice(0, probe.indexOf(':')), probe.slice(probe.indexOf(':') + 1)]
  if (!arg) return true
  try {
    if (kind === 'cmd') {
      // Daemons live in sbin, which is often off a user's PATH.
      const env = { ...process.env, PATH: `${process.env.PATH ?? ''}:/usr/sbin:/sbin` }
      await execFileAsync('which', [arg], { env })
      return true
    }
    if (kind === 'gst') {
      await execFileAsync('gst-inspect-1.0', ['--exists', arg])
      return true
    }
    if (kind === 'file') return pathPresent(arg)
    return true
  } catch {
    return false
  }
}

export async function missingPackages(required: PackageEntry[]): Promise<PackageEntry[]> {
  const checked = await Promise.all(
    required.map(async (e) => ({ entry: e, ok: await probeSatisfied(e.probe) }))
  )
  return checked.filter((c) => !c.ok).map((c) => c.entry)
}

function hasCommand(name: string): boolean {
  try {
    execFileSync('which', [name], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

type PackageManager = 'apt' | 'dnf'

/** Host package manager, apt before dnf. */
function packageManager(): PackageManager | null {
  if (hasCommand('apt-get')) return 'apt'
  if (hasCommand('dnf')) return 'dnf'
  return null
}

/** Package names for the manager, deduped, empties dropped. */
function installNames(missing: PackageEntry[], pm: PackageManager): string[] {
  const names = missing.map((e) => (pm === 'dnf' ? e.fedora : e.name)).filter((n) => n.length > 0)
  return [...new Set(names)]
}

function manualCommand(names: string[], pm: PackageManager): string {
  return pm === 'dnf'
    ? `sudo dnf install ${names.join(' ')}`
    : `sudo apt install ${names.join(' ')}`
}

function installPackages(names: string[], pm: PackageManager): Promise<void> {
  return new Promise((resolve, reject) => {
    const script =
      pm === 'dnf'
        ? `dnf install -y ${names.join(' ')}`
        : `apt-get update && apt-get install -y ${names.join(' ')}`
    const proc = spawn('pkexec', ['bash', '-c', script], { stdio: 'ignore' })
    proc.on('close', (code) => {
      if (code === 0) resolve()
      else reject(new Error(`pkexec exited with code ${code}`))
    })
    proc.on('error', reject)
  })
}

function describe(entries: PackageEntry[]): string {
  return entries.map((e) => (e.purpose ? `  ${e.name} — ${e.purpose}` : `  ${e.name}`)).join('\n')
}

export type PackageCheckResult = { dismissed?: string[] }

/**
 * Prompt once for every manifest package missing on this host, skipping the ones the user
 * has already declined. Returns the new dismissed list when the user opts out, else nothing.
 */
export async function checkMissingPackages(
  window: BrowserWindow,
  alreadyDismissed: string[]
): Promise<PackageCheckResult> {
  if (process.platform !== 'linux') return {}

  const required = requiredPackages()
  if (!required.length) return {}

  const declined = new Set(alreadyDismissed)
  const missing = (await missingPackages(required)).filter((e) => !declined.has(e.name))
  if (!missing.length) return {}

  // Dismissal keys on the Debian name; install uses the host PM's names.
  const dismissKeys = missing.map((e) => e.name)
  const pm = hasCommand('pkexec') ? packageManager() : null
  const canInstall = pm !== null
  const pkgNames = pm ? installNames(missing, pm) : dismissKeys

  const { response } = await dialog.showMessageBox(window, {
    type: 'question',
    title: 'LIVI — Missing Packages',
    message: `${missing.length} component${missing.length > 1 ? 's are' : ' is'} missing for a complete LIVI setup.`,
    detail:
      `${describe(missing)}\n\nLIVI runs without them, but the listed features stay unavailable.` +
      (pm
        ? '\n\nInstall?'
        : '\n\nNo supported package manager (apt/dnf) found — install the equivalents for your distro.'),
    // Later sits rightmost and is the cancel action: a reflex click on the far button or
    // an Esc defers instead of deciding, the next start asks again.
    buttons: canInstall ? ['Now', 'Never', 'Later'] : ['Never', 'Later'],
    defaultId: 0,
    cancelId: canInstall ? 2 : 1
  })

  if (!canInstall || !pm) {
    return response === 0 ? { dismissed: [...alreadyDismissed, ...dismissKeys] } : {}
  }
  if (response === 1) return { dismissed: [...alreadyDismissed, ...dismissKeys] }
  if (response !== 0) return {}

  try {
    await installPackages(pkgNames, pm)
    const stillMissing = await missingPackages(missing)
    if (stillMissing.length) {
      await dialog.showMessageBox(window, {
        type: 'warning',
        title: 'LIVI — Missing Packages',
        message: 'Some packages are still missing after the installation.',
        detail: `${describe(stillMissing)}\n\nRun this manually:\n\n${manualCommand(pkgNames, pm)}`,
        buttons: ['OK']
      })
      return {}
    }
    await dialog.showMessageBox(window, {
      type: 'info',
      title: 'LIVI — Missing Packages',
      message: 'All packages installed. Restart LIVI to use the features they enable.',
      buttons: ['OK']
    })
  } catch (err) {
    console.error('[packageCheck] installation failed:', err)
    await dialog.showMessageBox(window, {
      type: 'error',
      title: 'LIVI — Missing Packages',
      message: 'Could not install the packages.',
      detail: `${(err as Error).message}\n\nRun this manually:\n\n${manualCommand(pkgNames, pm)}`,
      buttons: ['OK']
    })
  }
  return {}
}
