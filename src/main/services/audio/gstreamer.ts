import { app } from 'electron'
import fs from 'fs'
import path from 'path'

function platformDir(): string | null {
  switch (process.platform) {
    case 'darwin':
      return process.arch === 'arm64' ? 'macos-arm64' : null
    case 'linux':
      return process.arch === 'arm64' ? 'linux-arm64' : process.arch === 'x64' ? 'linux-x64' : null
    case 'win32':
      return process.arch === 'x64' ? 'windows-x64' : null
    default:
      return null
  }
}

export function resolveGStreamerRoot(): string | null {
  const dir = platformDir()
  if (!dir) return null
  const base = app.isPackaged ? process.resourcesPath : path.join(app.getAppPath(), 'assets')
  const bundled = path.join(base, 'gstreamer', dir)
  return fs.existsSync(bundled) ? bundled : null
}

export function resolveBinary(name: 'gst-launch-1.0' | 'gst-device-monitor-1.0'): string | null {
  const root = resolveGStreamerRoot()
  if (!root) return null
  return path.join(root, 'bin', process.platform === 'win32' ? `${name}.exe` : name)
}

export function gstEnv(gstRoot: string): NodeJS.ProcessEnv {
  const pluginPath = path.join(gstRoot, 'lib', 'gstreamer-1.0')
  const pluginScanner = path.join(
    gstRoot,
    'libexec',
    'gstreamer-1.0',
    process.platform === 'win32' ? 'gst-plugin-scanner.exe' : 'gst-plugin-scanner'
  )
  const base = {
    ...process.env,
    GST_PLUGIN_SYSTEM_PATH: '',
    GST_PLUGIN_PATH: pluginPath,
    GST_PLUGIN_SCANNER: pluginScanner
  }
  if (process.platform === 'darwin') {
    return { ...base, DYLD_LIBRARY_PATH: path.join(gstRoot, 'lib') }
  }
  if (process.platform === 'linux') {
    return { ...base, LD_LIBRARY_PATH: path.join(gstRoot, 'lib') }
  }
  return { ...base, PATH: `${path.join(gstRoot, 'bin')};${process.env.PATH ?? ''}` }
}

export function audioSinkElement(): string {
  if (process.platform === 'darwin') return 'osxaudiosink'
  if (process.platform === 'win32') return 'wasapisink'
  return 'pulsesink'
}

export function audioSourceElement(): string {
  if (process.platform === 'darwin') return 'osxaudiosrc'
  if (process.platform === 'win32') return 'wasapisrc'
  return 'pulsesrc'
}

// pulsesink/pulsesrc: device=<string>
// osxaudiosink/osxaudiosrc: unique-id=<string>
// wasapisink/wasapisrc: device-name=<string>
export function audioDeviceProp(): 'device' | 'device-name' | 'unique-id' {
  if (process.platform === 'darwin') return 'unique-id'
  if (process.platform === 'win32') return 'device-name'
  return 'device'
}
