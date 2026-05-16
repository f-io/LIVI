import { DEBUG } from '@main/constants'
import { execFile } from 'child_process'
import { gstEnv, resolveBinary, resolveGStreamerRoot } from './gstreamer'

export type AudioDeviceKind = 'sink' | 'source'

export type AudioDevice = {
  // Verbatim value from the `device=` / `device-name=` parameter in the
  // gst-launch line that gst-device-monitor prints. Passed straight back
  // into pulsesink / wasapisink / osxaudiosink so the type (string vs uint)
  // is correct per platform.
  id: string
  // Human-friendly name for the dropdown
  name: string
  // True if the OS reports this as the default device
  isDefault: boolean
}

const ENUMERATE_TIMEOUT_MS = 4_000

export async function listAudioDevices(kind: AudioDeviceKind): Promise<AudioDevice[]> {
  const root = resolveGStreamerRoot()
  const bin = resolveBinary('gst-device-monitor-1.0')
  if (!root || !bin) {
    if (DEBUG) console.warn('[AudioDeviceEnumerator] GStreamer bundle missing')
    return []
  }

  const filter = kind === 'sink' ? 'Audio/Sink' : 'Audio/Source'

  return new Promise((resolve) => {
    execFile(
      bin,
      [filter],
      { env: gstEnv(root), timeout: ENUMERATE_TIMEOUT_MS, maxBuffer: 1024 * 1024 },
      (err, stdout) => {
        if (err) {
          if (DEBUG) console.warn('[AudioDeviceEnumerator] gst-device-monitor failed', err.message)
          resolve([])
          return
        }
        resolve(parseDeviceMonitorOutput(stdout, kind))
      }
    )
  })
}

export function parseDeviceMonitorOutput(stdout: string, kind: AudioDeviceKind): AudioDevice[] {
  const devices: AudioDevice[] = []
  const blocks = stdout.split(/^\s*Device found:\s*$/m)
  for (const block of blocks) {
    if (!block.trim()) continue

    const cls = matchProp(block, 'class')
    if (!cls) continue
    const expectedClass = kind === 'sink' ? 'Audio/Sink' : 'Audio/Source'
    if (!cls.includes(expectedClass)) continue

    // Skip PulseAudio/PipeWire monitor sources (loopback of a sink, not a
    // real capture device).
    if (kind === 'source' && matchProp(block, 'device.class') === 'monitor') continue

    const id =
      idFromLaunchLine(block) ??
      matchProp(block, 'unique-id') ??
      matchProp(block, 'device.name') ??
      matchProp(block, 'node.name') ??
      matchProp(block, 'alsa.card_name') ??
      matchProp(block, 'name')
    if (!id) continue

    const name =
      matchProp(block, 'name') ??
      matchProp(block, 'device.description') ??
      matchProp(block, 'node.description') ??
      matchProp(block, 'alsa.long_card_name') ??
      id

    const isDefault =
      /^\s*default:\s*true\s*$/im.test(block) ||
      matchProp(block, 'is-default') === 'true' ||
      matchProp(block, 'is-default') === 'true (gboolean)' ||
      matchProp(block, 'node.is-default') === 'true'

    devices.push({ id, name, isDefault })
  }

  const seen = new Set<string>()
  return devices.filter((d) => (seen.has(d.id) ? false : (seen.add(d.id), true)))
}

// gst-device-monitor prints a sample launch line per device, e.g.
//   gst-launch-1.0 ... ! osxaudiosink device=53
//   gst-launch-1.0 ... ! 'pulsesink device=alsa_output.platform-fef00700.hdmi.hdmi-stereo'
//   gst-launch-1.0 ... ! wasapisink device-name=\{0.0.0.00000000\}.\{abc-def\}
// We pull whatever follows device= / device-name= and stop at the first
// unquoted whitespace, end of line, or closing quote.
function idFromLaunchLine(block: string): string | null {
  const launch = block.match(/gst-launch-1\.0[^\n]*/m)
  if (!launch) return null
  const line = launch[0]
  // GStreamer 1.28+ uses unique-id on osxaudio; older / pulse / wasapi use
  // device or device-name. Catch all three.
  const m = line.match(/\b(?:unique-id|device-name|device)=("([^"]*)"|'([^']*)'|([^\s'"]+))/)
  if (!m) return null
  return m[2] ?? m[3] ?? m[4] ?? null
}

function matchProp(block: string, key: string): string | null {
  const escaped = key.replace(/[.[\]]/g, (c) => '\\' + c)
  const re = new RegExp(`^\\s*${escaped}\\s*[:=]\\s*(.+?)\\s*$`, 'm')
  const m = block.match(re)
  if (!m) return null
  return m[1].replace(/^"(.*)"$/, '$1')
}
