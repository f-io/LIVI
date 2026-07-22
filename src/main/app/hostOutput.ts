import { execFileSync } from 'node:child_process'

/**
 * The panel belongs to the host compositor (Cage in the kiosk), not to ours. Our own
 * WAYLAND_DISPLAY points at the nested compositor, so every query names the host.
 */
const HOST_DISPLAY = 'wayland-0'

function hostEnv(): NodeJS.ProcessEnv {
  return { ...process.env, WAYLAND_DISPLAY: HOST_DISPLAY }
}

function run(args: string[]): string | null {
  if (process.platform !== 'linux') return null
  try {
    return execFileSync('wlr-randr', args, { encoding: 'utf8', timeout: 3000, env: hostEnv() })
  } catch {
    return null
  }
}

/** Name of the host's first output, or null when it cannot be reached. */
export function hostOutputName(): string | null {
  const listed = run([])
  const name = listed?.split('\n')[0]?.trim().split(/\s+/)[0]
  return name || null
}

/** Modes the panel offers as "WIDTHxHEIGHT", widest first, without duplicates. */
export function listHostOutputModes(): string[] {
  const listed = run([])
  if (!listed) return []
  const seen = new Set<string>()
  for (const line of listed.split('\n')) {
    const m = line.trim().match(/^(\d+)x(\d+) px/)
    if (m) seen.add(`${m[1]}x${m[2]}`)
  }
  return [...seen].sort((a, b) => {
    const [aw, ah] = a.split('x').map(Number)
    const [bw, bh] = b.split('x').map(Number)
    return bw * bh - aw * ah
  })
}

/**
 * Put the panel into the given mode, given as "WIDTHxHEIGHT". An empty mode leaves the
 * display at whatever it came up in. A mode the panel does not offer is refused by
 * wlr-randr and leaves it untouched.
 */
export function applyHostOutputMode(mode: string): void {
  if (!/^\d+x\d+$/.test(mode)) return
  const name = hostOutputName()
  if (!name) {
    console.warn('[hostOutput] no host output found, leaving the panel at its own mode')
    return
  }
  if (run(['--output', name, '--mode', mode]) === null) {
    console.warn(`[hostOutput] ${name} does not take ${mode}, leaving it as it is`)
    return
  }
  console.log(`[hostOutput] ${name} → ${mode}`)
}
