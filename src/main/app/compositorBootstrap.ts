import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { COMPOSITOR_TITLEBAR_H } from '@main/app/compositorLayout'
import { loadConfig } from '@main/config/loadConfig'

// Linux windowed (GNOME/labwc): host the UI plus the GStreamer video plane in
// the nested wlroots compositor so they composite into one window, zero-copy :)
export function bootstrapCompositor(): boolean {
  if (process.platform !== 'linux') return false
  if (process.env.LIVI_COMPOSITOR === '1') return false
  if (process.env.LIVI_NO_COMPOSITOR === '1') return false

  // Only the AppImage has a stable self-path to re-launch
  const appImage = process.env.APPIMAGE
  if (!appImage) return false

  const launcher = join(process.resourcesPath, 'compositor', 'livi-compositor')
  if (!existsSync(launcher)) return false

  // The inner AppImage must re-mount fresh (drop AppRun's vars)
  const hostLd = process.env.LD_LIBRARY_PATH ?? ''
  const inner =
    `LIVI_COMPOSITOR=1 LD_LIBRARY_PATH='${hostLd}' ` + `'${appImage}' --ozone-platform=wayland`

  // Control socket: the host drives screen outputs + video placement/crop/visibility over this
  const runtimeDir = process.env.XDG_RUNTIME_DIR || '/tmp'
  const ctrlSocket = join(runtimeDir, 'livi-compositor.ctrl')

  let outputSize: string | undefined
  try {
    const cfg = loadConfig()
    const ow = Math.round(Number(cfg.displayWidth))
    const oh = Math.round(Number(cfg.displayHeight))
    const wantKiosk = cfg.kiosk?.main === true || process.env.LIVI_KIOSK === '1'
    if (ow > 0 && oh > 0) outputSize = `${ow}x${oh + (wantKiosk ? 0 : COMPOSITOR_TITLEBAR_H)}`
  } catch {
    // fall back to the compositor's built-in default
  }

  // Known screen roles; the host opens/closes each output on demand via the control socket
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    LIVI_UI_APP_ID: 'livi',
    LIVI_OUTPUT_APP_ID: 'livi',
    LIVI_COMPOSITOR_CTRL: ctrlSocket,
    LIVI_SCREENS: 'main,dash,aux',
    ...(outputSize ? { LIVI_OUTPUT_SIZE: outputSize } : {})
  }
  delete env.APPIMAGE
  delete env.APPDIR
  delete env.ARGV0
  delete env.OWD

  spawn(launcher, ['-s', inner], { detached: true, stdio: 'inherit', env }).unref()
  return true
}
