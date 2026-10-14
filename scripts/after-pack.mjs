// Linux: AppRun runs the app's executable, so Electron moves aside as livi-ui
// and a start script for livi-core takes its name. Core then starts Electron.
// macOS: gst-host and the video addon find the bundled GStreamer through their
// rpath, since the hardened runtime drops DYLD_LIBRARY_PATH.
import { execFileSync } from 'node:child_process'
import { chmod, rename, writeFile } from 'node:fs/promises'
import { join } from 'node:path'

// AppRun passes Electron's sandbox flags, core hands them on to the UI. `LIVI.AppImage telemetry`
// pushes into the running LIVI instead of starting a second one, AppRun may put --no-sandbox first.
const START = `#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
[ "$1" = --no-sandbox ] && [ "$2" = telemetry ] && shift
[ "$1" = telemetry ] && exec "$HERE/resources/core/livi-core" "$@"
export LIVI_RESOURCES="$HERE/resources"
export LIVI_UI_ARGS="$*"
exec "$HERE/resources/core/livi-core"
`

const MAC_GSTREAMER_LIB = '@loader_path/../gstreamer/macos-arm64/lib'
const MAC_LINKS_GSTREAMER = ['gst-host/livi-gst-host', 'video/gst_video.node']

async function linux(context) {
  const dir = context.appOutDir
  const exe = join(dir, context.packager.executableName)
  await rename(exe, join(dir, 'livi-ui'))
  await writeFile(exe, START)
  await chmod(exe, 0o755)
}

/** Among them the build machine's GStreamer framework, which the app must not load. */
function rpaths(file) {
  const out = execFileSync('otool', ['-l', file], { encoding: 'utf8' })
  return [...out.matchAll(/cmd LC_RPATH\n\s+cmdsize \d+\n\s+path (.+) \(offset \d+\)/g)].map(
    (m) => m[1]
  )
}

function mac(context) {
  const app = join(context.appOutDir, `${context.packager.appInfo.productFilename}.app`)
  for (const rel of MAC_LINKS_GSTREAMER) {
    const file = join(app, 'Contents', 'Resources', rel)
    const drop = rpaths(file).flatMap((p) => ['-delete_rpath', p])
    execFileSync('install_name_tool', [...drop, '-add_rpath', MAC_GSTREAMER_LIB, file])
    console.log(`[after-pack] ${rel} links the bundled GStreamer`)
  }
}

export async function afterPack(context) {
  if (context.electronPlatformName === 'linux') await linux(context)
  if (context.electronPlatformName === 'darwin') mac(context)
}
